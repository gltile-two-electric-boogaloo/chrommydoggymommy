use rkyv::{Archive, Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Archive, Deserialize, Serialize, Debug)]
pub enum JobResult {
    Succeeded {
        freq: HashMap<usize, usize>,
        time: usize,
    },

    Failed {
        why: String,
        time: usize,
    },
}

#[derive(Archive, Deserialize, Serialize, Debug)]
pub enum C2SMessage {
    /// Log a message to the user's screen.
    Log(String),

    /// Report progress to the server.
    JobProgress {
        job_handle: usize,
        iterations_completed: usize,
    },

    /// Report the result of a job to the server.
    JobResult {
        job_handle: usize,
        result: JobResult,
    },

    /// Request an extra job handle so clients can give more fine-grained progress bars.
    RequestExtraJobHandle { task: String, max: usize },
}

#[derive(Archive, Deserialize, Serialize, Debug)]
pub enum S2CMessage {
    /// Send a job to the client.
    Job {
        job_handle: usize,
        iterations: usize,
        ports: usize,
        program: String,
    },

    /// Response to extra job handle request.
    ExtraJobHandle { job_handle: usize },

    /// Tell the client to exit.
    Finish,
}
