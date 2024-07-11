use crate::structs::{C2SMessage, JobResult, S2CMessage};
use anyhow::anyhow;
use byteorder::{NativeEndian, ReadBytesExt, WriteBytesExt};
use pyo3::exceptions::{PyEnvironmentError, PyTypeError};
use pyo3::prelude::PyModule;
use pyo3::prelude::*;
use pyo3::types::{PyCFunction, PyDict, PyTuple};
use pyo3::{Bound, PyErr, PyResult, Python};
use rand::prelude::SliceRandom;
use rand::thread_rng;
use rkyv::ser::serializers::AllocSerializer;
use rkyv::{Archive, Deserialize, Infallible, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing_mutex::stdsync::{Mutex, OnceLock, RwLock};

trait MessageSendExt {
    fn send<T>(&mut self, message: T) -> anyhow::Result<()>
    where
        T: Serialize<AllocSerializer<256>>;
}

impl<W> MessageSendExt for W
where
    W: Write,
{
    fn send<T>(&mut self, message: T) -> anyhow::Result<()>
    where
        T: Serialize<AllocSerializer<256>>,
    {
        let msg_buf = rkyv::to_bytes::<T, 256>(&message)?;
        self.write_u32::<NativeEndian>(msg_buf.len() as u32)?;
        self.write_all(&*msg_buf)?;
        self.flush()?;

        Ok(())
    }
}

trait MessageRecvExt {
    fn receive<T>(&mut self) -> anyhow::Result<T>
    where
        T: Archive,
        T::Archived: Deserialize<T, Infallible>;
}

impl<R> MessageRecvExt for R
where
    R: Read,
{
    fn receive<T>(&mut self) -> anyhow::Result<T>
    where
        T: Archive,
        T::Archived: Deserialize<T, Infallible>,
    {
        let len = self.read_u32::<NativeEndian>()?;
        let mut buf = vec![0u8; len as usize];
        self.read_exact(buf.as_mut_slice())?;

        let message = unsafe { rkyv::archived_root::<T>(&*buf) };

        // unwrap is okay because it is infallible
        Ok(message.deserialize(&mut Infallible).unwrap())
    }
}

/// You'll write a Python 3.12 program that solves this problem since it's too urgent to do by
/// hand. It'll be a function sort(n) that accepts how many cables/ports there are and employs
/// the following helper functions you'll be provided:
///
/// - query() is a function that takes no arguments and returns how many cables are currently
///   in the wrong place. 0 means that you have found the correct order and your function may
///   now return.
///
/// - swap(i, j) is a function that takes two 0-indexed integers and swaps the corresponding
///   cables. It has no return value, and you must explicitly use query() to see its effect.
///
/// Your algorithm will be tested on many randomized port configurations that may number
/// hundreds of cables, so it has to stay general. Winner is the one who minimizes average swaps
/// on a certain size of port configurations, e.g., 17. It's not going to actually be 17; we're
/// not telling you.
///
/// Also, we don't have the budget for any Python modules. If I see an eval(), exec(), or
/// compile(), I'll personally punch you in the face.
fn run_program(program: &str, ports: usize) -> anyhow::Result<usize> {
    let job_handle: OnceLock<usize> = OnceLock::new();
    let last_time = Arc::new(RwLock::new(Instant::now()));

    Python::with_gil(|py| {
        let (current_order, current_wrong) = {
            let mut order: Vec<usize> = (0..ports).collect();
            order.shuffle(&mut thread_rng());

            let mut wrong = 0;

            for (correct, current) in order.iter().enumerate() {
                if correct != *current {
                    wrong += 1;
                }
            }

            (
                Arc::new(Mutex::new(order)),
                Arc::new(AtomicUsize::new(wrong)),
            )
        };

        let swaps = Arc::new(AtomicUsize::new(0));

        let query = {
            let current_wrong = current_wrong.clone();

            move |args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>| -> PyResult<_> {
                if args.len() != 0 || _kwargs.is_some() {
                    return Err(PyErr::new::<PyTypeError, _>(
                        "query expects no arguments",
                    ));
                }
                
                Ok(current_wrong.load(Ordering::SeqCst))
            }
        };

        let swap = {
            let current_order = current_order.clone();
            let swaps = swaps.clone();

            move |args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>| -> PyResult<_> {
                if _kwargs.is_some() {
                    return Err(PyErr::new::<PyTypeError, _>(
                        "swap expects no keyword arguments",
                    ));
                }

                let (first, second) = args.extract::<(usize, usize)>()?;

                {
                    let mut current_order = current_order.lock().unwrap();
                    let first_item = current_order[first];
                    current_order[first] = current_order[second];
                    current_order[second] = first_item;

                    let mut delta = current_wrong.load(Ordering::SeqCst);

                    if first == current_order[first] {
                        // first has been moved into position
                        delta -= 1;
                    }

                    if first == current_order[second] {
                        // first has been moved out of position
                        delta += 1;
                    }

                    if second == current_order[second] {
                        // second has been moved into position
                        delta -= 1;
                    }

                    if second == current_order[first] {
                        // second has been moved out of position
                        delta += 1;
                    }

                    current_wrong.store(delta, Ordering::SeqCst);
                }

                swaps
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |x| Some(x + 1))
                    .unwrap();

                if last_time.read().unwrap().elapsed().as_secs() >= 1 {
                    *last_time.write().unwrap() = Instant::now();

                    let handle = job_handle.get_or_init(|| {
                        std::io::stdout()
                            .send(C2SMessage::RequestExtraJobHandle {
                                max: 0,
                                task: "Running iteration...".into(),
                            })
                            .unwrap();
                        let S2CMessage::ExtraJobHandle { job_handle } =
                            std::io::stdin().receive().unwrap()
                        else {
                            panic!("Received unexpected message from server")
                        };

                        job_handle
                    });

                    std::io::stdout()
                        .send(C2SMessage::JobProgress {
                            job_handle: *handle,
                            iterations_completed: swaps.load(Ordering::SeqCst),
                        })
                        .unwrap();
                }

                Ok(())
            }
        };

        let print =
            move |args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>| -> PyResult<_> {
                std::io::stdout()
                    .send(C2SMessage::Log(args.repr()?.to_str()?.into()))
                    .map_err(|e| {
                        PyErr::new::<PyEnvironmentError, _>(format!("Failed to print: {e}"))
                    })?;
                Ok(())
            };

        let user_module = PyModule::from_code_bound(py, program, "", "")?;
        let function = user_module.getattr("sort")?;
        let fn_env = function.getattr("__globals__")?;

        fn_env.set_item(
            "query",
            PyCFunction::new_closure_bound(py, None, None, query)?,
        )?;
        fn_env.set_item(
            "swap",
            PyCFunction::new_closure_bound(py, None, None, swap)?,
        )?;
        fn_env.set_item(
            "print",
            PyCFunction::new_closure_bound(py, None, None, print)?,
        )?;
        function.call1((ports,))?;

        {
            let current_order = current_order.lock().unwrap();
            for (correct, current) in current_order.iter().enumerate() {
                if correct != *current {
                    return Err(anyhow!(
                        "sort function did not sort: array was {:#?}",
                        current_order
                    ));
                }
            }
        }

        Ok(swaps.load(Ordering::SeqCst))
    })
}

pub(crate) fn main_child() -> anyhow::Result<()> {
    let mut stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    pyo3::prepare_freethreaded_python();

    loop {
        let message = stdin.receive()?;
        match message {
            S2CMessage::Finish => return Ok(()),
            S2CMessage::Job {
                job_handle,
                iterations,
                program,
                ports,
            } => {
                let prog_ref = &*program;
                let mut freq_map = HashMap::new();

                let stime = Instant::now();
                for it in 0..iterations {
                    match run_program(prog_ref, ports) {
                        Ok(swaps) => *freq_map.entry(swaps).or_insert(0) += 1,
                        Err(e) => {
                            stdout.send(C2SMessage::JobResult {
                                job_handle,
                                result: JobResult::Failed {
                                    time: stime.elapsed().as_millis() as usize,
                                    why: e.to_string(),
                                },
                            })?;

                            continue;
                        }
                    };

                    stdout.send(C2SMessage::JobProgress {
                        job_handle,
                        iterations_completed: it,
                    })?;
                }

                stdout.send(C2SMessage::JobResult {
                    job_handle,
                    result: JobResult::Succeeded {
                        // As long as it doesn't take more than 584,554,531 years to run,
                        // this is okay :)
                        time: stime.elapsed().as_millis() as usize,
                        freq: freq_map,
                    },
                })?;
            }
            _ => panic!("Recieved unexpected client message {message:#?}"),
        }
    }
}
