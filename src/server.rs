use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use async_trait::async_trait;
use byteorder::{NativeEndian, ReadBytesExt, WriteBytesExt};
use futures_util::future::{join_all, select_all};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use lazy_static::lazy_static;
use rkyv::{Archive, Deserialize, Infallible, Serialize};
use rkyv::ser::serializers::AllocSerializer;
use tokio::fs::read;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::join;
use tokio::process::{Child, Command};
use tokio::runtime::Runtime;
use tokio_stream as stream;
use tokio::sync::Mutex;
use tokio_stream::StreamExt;
use crate::{C2SMessage, JobResult, S2CMessage};

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

struct Job {
    name: String,
    program: String,
    iterations: usize,
    ports: usize,
}

#[async_trait]
trait AsyncMessageSendExt {
    async fn send<T>(&mut self, message: T) -> anyhow::Result<()>
    where
        T: Serialize<AllocSerializer<256>> + Send;
}

#[async_trait]
impl<W> AsyncMessageSendExt for W
where
    W: AsyncWrite + Unpin + Send,
{
    async fn send<T>(&mut self, message: T) -> anyhow::Result<()>
    where
        T: Serialize<AllocSerializer<256>> + Send,
    {
        let msg_buf = rkyv::to_bytes::<T, 256>(&message)?;
        self.write_u32_le(msg_buf.len() as u32).await?;
        self.write_all(&*msg_buf).await?;
        self.flush().await?;

        Ok(())
    }
}

#[async_trait]
trait AsyncMessageRecvExt {
    async fn receive<T>(&mut self) -> anyhow::Result<T>
    where
        T: Archive,
        T::Archived: Deserialize<T, Infallible> + Send;
}

#[async_trait]
impl<R> AsyncMessageRecvExt for R
where
    R: AsyncRead + Unpin + Send,
{
    async fn receive<T>(&mut self) -> anyhow::Result<T>
    where
        T: Archive,
        T::Archived: Deserialize<T, Infallible>,
    {
        let len = self.read_u32_le().await?;
        let mut buf = vec![0u8; len as usize];
        self.read_exact(buf.as_mut_slice()).await?;

        let message = unsafe { rkyv::archived_root::<T>(&*buf) };

        // unwrap is okay because it is infallible
        Ok(message.deserialize(&mut Infallible).unwrap())
    }
}

pub(crate) fn main_server(workers: usize, iterations: usize, ports: usize, files: Vec<String>, child_binary: &str) -> anyhow::Result<()> {
    let runtime = Runtime::new()?;

    runtime.block_on(main_server_async(workers, iterations, ports, files, child_binary))
}

async fn main_server_async(workers: usize, iterations: usize, ports: usize, files: Vec<String>, child_binary: &str) -> anyhow::Result<()> {
    let mut worker_pool = Vec::new();
    let mut jobs = Vec::new();
    let log = Arc::new(MultiProgress::new());

    for name in files {
        let file = String::from_utf8(read(&*name).await?)?;
        jobs.push(Job {
            program: file,
            iterations,
            ports,
            name
        })
    }

    let jobs = Arc::new(Mutex::new(stream::iter(jobs)));

    for _ in 0..workers {
        let child = Command::new(child_binary)
            .arg("--run-child-process")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;

        let worker = tokio::spawn(program_task(jobs.clone(), child, log.clone()));
        worker_pool.push(worker)
    }

    while worker_pool.len() != 0 {
        let fut;
        (fut, _, worker_pool) = select_all(worker_pool).await;

        fut?;
    }

    Ok(())
}

async fn program_task(jobs: Arc<Mutex<stream::Iter<<Vec<Job> as IntoIterator>::IntoIter>>>, mut child: Child, log: Arc<MultiProgress>) -> anyhow::Result<()> {
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut current_handle_index = 1;
    let mut handles: HashMap<usize, ProgressBar> = HashMap::new();

    while let Some(job) = jobs.lock().await.next().await {
        let job_progress = ProgressBar::new(job.iterations as u64);
        job_progress.set_prefix(job.name.clone());
        job_progress.set_message("Running iterations...");
        job_progress.set_style(PROGRESS_STYLE.clone());
        job_progress.tick();
        stdin.send(S2CMessage::Job {
            job_handle: 0,
            iterations: job.iterations,
            ports: job.ports,
            program: job.program,
        }).await?;

        loop {
            let message = stdout.receive().await?;
            match message {
                C2SMessage::Log(message) => log.println(message)?,
                C2SMessage::JobProgress {job_handle, iterations_completed} => {
                    if job_handle == 0 {
                        job_progress.set_position(iterations_completed as u64)
                    } else {
                        handles.get(&job_handle).unwrap().set_position(iterations_completed as u64)
                    }
                }
                C2SMessage::JobResult {job_handle, result} => {
                    if job_handle == 0 {
                        match result {
                            JobResult::Succeeded {freq, time} => {
                                job_progress.set_style(COMPLETED_STYLE.clone());
                                job_progress.finish_with_message(format!("Finished in {} ms", time));
                            }
                            JobResult::Failed {why, time} => {
                                job_progress.set_style(FAILED_STYLE.clone());
                                job_progress.finish_with_message(format!("Failed in {} ms: {}", time, why));
                            }
                        }
                    } else {
                        match result {
                            JobResult::Succeeded {freq, time} => {
                                let handle = handles.get(&job_handle).unwrap();
                                handle.set_style(COMPLETED_STYLE.clone());
                                handle.finish_and_clear()
                            }
                            JobResult::Failed {why, time} => {
                                let handle = handles.get(&job_handle).unwrap();
                                handle.set_style(FAILED_STYLE.clone());
                                handles.get(&job_handle).unwrap().finish_with_message(format!("Failed in {} ms: {}", time, why))
                            }
                        }
                    }
                }
                C2SMessage::RequestExtraJobHandle {task, max} => {
                    let prog = ProgressBar::new(max as u64);
                    prog.set_prefix(job.name.clone());
                    prog.set_message(task);

                    if max == 0 {
                        prog.set_style(SPINNER_STYLE.clone());
                    } else {
                        prog.set_style(PROGRESS_STYLE.clone());
                    }

                    handles.insert(current_handle_index, prog);

                    stdin.send(S2CMessage::ExtraJobHandle {
                        job_handle: current_handle_index
                    }).await?;

                    current_handle_index += 1;
                }
            }
        }
    }

    stdin.send(S2CMessage::Finish).await?;
    child.wait().await?;

    Ok(())
}