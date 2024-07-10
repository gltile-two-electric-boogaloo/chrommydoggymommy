use clap::Parser;
use std::env;
use std::error::Error;

use crate::client::main_child;
use crate::server::main_server;

mod client;
mod server;
mod structs;

#[derive(Parser, Debug)]
#[command(about = "Overengineered test runner")]
struct Args {
    /// Amount of workers to run (default: CPU cores)
    #[arg(short = 'n', default_value = None)]
    workers: Option<usize>,

    /// How many times each program will run
    #[arg(short = 'i')]
    iterations: usize,

    /// How many ports
    #[arg(short = 'p')]
    ports: usize,

    /// Batching size (default: based on number of ports)
    #[arg(short = 'b')]
    batch_size: Option<usize>,

    /// Checkpoint file
    #[arg(short = 'c')]
    checkpoint_file: String,

    /// Files to run
    files: Vec<String>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut arg_name: Vec<String> = env::args().collect();
    if arg_name.len() == 2 && arg_name[1].as_str() == "--run-child-process" {
        return main_child();
    }

    let args = Args::parse();
    let workers = args.workers.or(Some(num_cpus::get())).unwrap();
    let batch_size = args
        .batch_size
        .or(Some((1f64 / args.ports as f64 / 100f64).ceil() as usize))
        .unwrap();

    Ok(main_server(
        workers,
        args.iterations,
        args.ports,
        args.files,
        batch_size,
        args.checkpoint_file,
        &*arg_name[0],
    )?)
}
