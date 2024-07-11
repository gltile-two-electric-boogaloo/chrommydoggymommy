mod traits;

use crate::checkpoint::{Checkpoint, CheckpointEntry};
use crate::server::traits::{AsyncMessageRecvExt, AsyncMessageSendExt};
use crate::structs::{C2SMessage, JobResult, S2CMessage};
use anyhow::bail;
use futures_util::future::select_all;
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use lazy_static::lazy_static;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::fs::read;
use tokio::io;
use tokio::process::{Child, Command};
use tokio::runtime::Runtime;
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::sync::{broadcast, Mutex, RwLock};

lazy_static! {
    static ref PROGRESS_STYLE: ProgressStyle =
        ProgressStyle::with_template("{spinner} {prefix} {msg:.dim} {human_pos:>7} {bar:20.cyan/dim} {human_len:<7} {percent:>3}% ({per_sec} it/sec)")
            .unwrap()
            .progress_chars("━╸━")
            .tick_chars("⠟⠯⠷⠾⠽⠻ ");
    static ref SPINNER_STYLE: ProgressStyle =
        ProgressStyle::with_template("{spinner} {prefix} {msg:.dim} {human_pos:>7} ({per_sec} ev/sec)")
            .unwrap()
            .tick_chars("⠟⠯⠷⠾⠽⠻ ");
    static ref COMPLETED_STYLE: ProgressStyle =
        ProgressStyle::with_template("⠿ {prefix} {msg:.green} {human_pos:>7}")
            .unwrap();
    static ref FAILED_STYLE: ProgressStyle =
        ProgressStyle::with_template("x {prefix} {msg:.red} {human_pos:>7}")
            .unwrap();
}

#[derive(Clone, Debug)]
enum JobProgress {
    Progress {
        id: usize,
        progress: usize,
    },
    Finished {
        id: usize,
        progress: usize,
        time: usize,
        frequencies: HashMap<usize, usize>,
    },
    Failed {
        id: usize,
        progress: usize,
        why: String,
        time: usize,
    },
}

struct Job {
    name: String,
    program: String,
    iterations: usize,
    ports: usize,
    id: usize,
    report: Arc<Sender<JobProgress>>,
}

pub(crate) fn main_server(
    workers: usize,
    iterations: usize,
    ports: usize,
    files: Vec<String>,
    batch_size: usize,
    checkpoint_file: String,
    child_binary: &str,
) -> anyhow::Result<()> {
    let runtime = Runtime::new()?;

    runtime.block_on(main_server_async(
        workers,
        iterations,
        ports,
        files,
        batch_size,
        checkpoint_file,
        child_binary,
    ))
}

async fn main_server_async(
    workers: usize,
    iterations: usize,
    ports: usize,
    files: Vec<String>,
    batch_size: usize,
    checkpoint_file: String,
    child_binary: &str,
) -> anyhow::Result<()> {
    let mut tasks = Vec::new();
    let log = Arc::new(MultiProgress::new());
    log.set_draw_target(ProgressDrawTarget::stdout_with_hz(5));
    let checkpoint: Arc<RwLock<Checkpoint>> =
        Arc::new(RwLock::new(
            match Checkpoint::read_async(checkpoint_file.clone()).await {
                Ok(checkpoint) => checkpoint,
                Err(e) => {
                    if let Some(e) = e.downcast_ref::<io::Error>() {
                        if e.kind() == io::ErrorKind::NotFound {
                            log.println(format!(
                                "Checkpoint file {checkpoint_file} not found, making new one"
                            ))?;
                            Checkpoint {
                                time: SystemTime::now()
                                    .duration_since(SystemTime::UNIX_EPOCH)
                                    .unwrap()
                                    .as_secs(),
                                algorithms: HashMap::new(),
                            }
                        } else {
                            bail!("Unable to read checkpoint file: {e}")
                        }
                    } else {
                        bail!("Unable to read checkpoint file: {e}")
                    }
                }
            },
        ));

    let jobs = {
        let mut jobs = Vec::new();

        for name in files {
            let file = String::from_utf8(read(&*name).await?)?;
            let digest = Sha256::digest(&*file).to_vec();

            let already_computed = checkpoint
                .read()
                .await
                .algorithms
                .get(&*digest)
                .map(|entry| {
                    entry
                        .frequency_map
                        .get(&ports)
                        .map(|freq| freq.values().sum::<usize>())
                        .unwrap_or(0)
                })
                .unwrap_or(0);

            if already_computed >= iterations {
                log.println(format!("Skipping {name}, checkpoint file already contains amount of iterations requested (requested {iterations}, have {already_computed})"))?;
                continue;
            }

            let iterations = iterations - already_computed;

            let (tx, rx): (Sender<JobProgress>, Receiver<JobProgress>) =
                broadcast::channel((iterations / batch_size) + 1);
            let tx = Arc::new(tx);

            for id in 0..iterations / batch_size {
                jobs.push(Job {
                    program: file.clone(),
                    iterations: batch_size,
                    ports,
                    id,
                    report: tx.clone(),
                    name: name.clone(),
                })
            }

            if (iterations % batch_size) != 0 {
                jobs.push(Job {
                    program: file.clone(),
                    iterations: iterations % batch_size,
                    ports,
                    id: iterations / batch_size,
                    report: tx.clone(),
                    name: name.clone(),
                })
            }

            tasks.push(tokio::spawn(job_supervisor(
                rx,
                log.clone(),
                iterations,
                ports,
                iterations.div_ceil(batch_size),
                name.clone(),
                checkpoint.clone(),
                checkpoint_file.clone(),
                digest,
            )));
        }

        Arc::new(Mutex::new(jobs))
    };

    for _ in 0..workers {
        let child = Command::new(child_binary)
            .arg("--run-child-process")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;

        let worker = tokio::spawn(worker(jobs.clone(), child, log.clone()));
        tasks.push(worker)
    }

    while tasks.len() != 0 {
        let fut;
        (fut, _, tasks) = select_all(tasks).await;
        fut??;
    }

    Ok(())
}

async fn job_supervisor(
    mut recv: Receiver<JobProgress>,
    log: Arc<MultiProgress>,
    iterations: usize,
    ports: usize,
    jobs: usize,
    name: String,
    checkpoint: Arc<RwLock<Checkpoint>>,
    checkpoint_file: String,
    digest: Vec<u8>,
) -> anyhow::Result<()> {
    let mut progresses = vec![0usize; jobs];
    let mut progress = 0usize;
    let progress_bar = log.add(ProgressBar::new(iterations as u64));
    let mut total_time = 0usize;
    let mut frequencies = HashMap::new();
    progress_bar.set_prefix(name.clone());
    progress_bar.set_message("Running iterations...");
    progress_bar.set_style(PROGRESS_STYLE.clone());
    progress_bar.tick();

    let mut error_bar: Option<ProgressBar> = None;

    while progress != iterations {
        let message = recv.recv().await?;
        match message {
            JobProgress::Progress {
                progress: job_progress,
                id,
            } => {
                progresses[id] = job_progress;
                progress = progresses.iter().sum();
                progress_bar.set_position(progress as u64);
            }
            JobProgress::Finished {
                progress: job_progress,
                id,
                time,
                frequencies: job_frequencies,
            } => {
                progresses[id] = job_progress;
                progress = progresses.iter().sum();
                progress_bar.set_position(progress as u64);
                total_time += time;
                for (time, freq) in job_frequencies {
                    *frequencies.entry(time).or_insert(0) += freq
                }
            }
            JobProgress::Failed {
                id,
                why,
                time,
                progress,
            } => {
                progresses[id] = progress;
                total_time += time;

                if let Some(ref error_bar) = error_bar {
                    error_bar.inc(1);
                } else {
                    progress_bar.set_style(FAILED_STYLE.clone());
                    progress_bar
                        .finish_with_message(format!("A worker failed in {} ms: {}", time, why));

                    error_bar = Some({
                        let error_bar = log.add(ProgressBar::new(jobs as u64));
                        error_bar.set_position(1);
                        error_bar.set_prefix(name.clone());
                        error_bar.set_message("Waiting for workers to stop...");
                        error_bar.set_style(PROGRESS_STYLE.clone());
                        error_bar.tick();

                        error_bar
                    });
                }
            }
        }
    }

    if let Some(ref error_bar) = error_bar {
        error_bar.finish_and_clear();
        progress_bar.set_style(FAILED_STYLE.clone());
        progress_bar.set_message(format!("Failed in {total_time} ms"));
    } else {
        progress_bar.set_style(COMPLETED_STYLE.clone());
        progress_bar.finish_with_message(format!("Completed in {total_time} ms"));

        let mut buf = [0u8; 32];
        buf.copy_from_slice(&*digest);
        let mut checkpoint = checkpoint.write().await;
        let entry = checkpoint.algorithms.entry(buf).or_insert(CheckpointEntry {
            name,
            frequency_map: HashMap::new(),
        });
        let map = entry.frequency_map.entry(ports).or_insert(HashMap::new());
        for (time, freq) in frequencies {
            *map.entry(time).or_insert(0) += freq
        }
        checkpoint.write_async(checkpoint_file).await?;
    }

    Ok(())
}

async fn worker(
    jobs: Arc<Mutex<Vec<Job>>>,
    mut child: Child,
    log: Arc<MultiProgress>,
) -> anyhow::Result<()> {
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut current_handle_index = 1;
    let mut handles: HashMap<usize, ProgressBar> = HashMap::new();

    loop {
        let Some(job) = ({
            let mut lock = jobs.lock().await;
            lock.pop()
        }) else {
            break;
        };

        stdin
            .send(S2CMessage::Job {
                job_handle: 0,
                iterations: job.iterations,
                ports: job.ports,
                program: job.program,
            })
            .await?;

        loop {
            let message = stdout.receive().await?;
            match message {
                C2SMessage::Log(message) => log.println(message)?,
                C2SMessage::JobProgress {
                    job_handle,
                    iterations_completed,
                } => {
                    if job_handle == 0 {
                        job.report.send(JobProgress::Progress {
                            id: job.id,
                            progress: iterations_completed,
                        })?;
                    } else {
                        handles
                            .get(&job_handle)
                            .unwrap()
                            .set_position(iterations_completed as u64)
                    }
                }
                C2SMessage::JobResult { job_handle, result } => {
                    if job_handle == 0 {
                        match result {
                            JobResult::Succeeded { freq, time } => {
                                job.report.send(JobProgress::Finished {
                                    id: job.id,
                                    progress: job.iterations,
                                    time,
                                    frequencies: freq,
                                })?;
                            }
                            JobResult::Failed { why, time } => {
                                job.report.send(JobProgress::Failed {
                                    id: job.id,
                                    progress: job.iterations,
                                    why,
                                    time,
                                })?;
                            }
                        }

                        break;
                    } else {
                        match result {
                            JobResult::Succeeded { .. } => {
                                let handle = handles.get(&job_handle).unwrap();
                                handle.set_style(COMPLETED_STYLE.clone());
                                handle.finish_and_clear()
                            }
                            JobResult::Failed { why, time } => {
                                let handle = handles.get(&job_handle).unwrap();
                                handle.set_style(FAILED_STYLE.clone());
                                handles
                                    .get(&job_handle)
                                    .unwrap()
                                    .finish_with_message(format!("Failed in {} ms: {}", time, why))
                            }
                        }
                    }
                }
                C2SMessage::RequestExtraJobHandle { task, max } => {
                    let prog = log.add(ProgressBar::new(max as u64));
                    prog.set_prefix(job.name.clone());
                    prog.set_message(task);

                    if max == 0 {
                        prog.set_style(SPINNER_STYLE.clone());
                    } else {
                        prog.set_style(PROGRESS_STYLE.clone());
                    }

                    handles.insert(current_handle_index, prog);

                    stdin
                        .send(S2CMessage::ExtraJobHandle {
                            job_handle: current_handle_index,
                        })
                        .await?;

                    current_handle_index += 1;
                }
            }
        }
    }

    stdin.send(S2CMessage::Finish).await?;
    child.wait().await?;

    Ok(())
}
