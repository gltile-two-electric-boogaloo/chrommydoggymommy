use clap::{Parser, Subcommand};
use std::env;
use crate::checkpoint_cli::main_checkpoint_cli;
use crate::client::main_child;
use crate::server::main_server;

mod client;
mod server;
mod structs;
mod checkpoint;
mod checkpoint_cli;

#[derive(Parser, Debug)]
#[command(about = "Overengineered test runner")]
struct Args {
    #[command(subcommand)]
    command: Command
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Test a program or a set of programs.
    Run {
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
    },
    
    /// Work with checkpoint files.
    Checkpoint {
        #[command(subcommand)]
        command: checkpoint_cli::Command
    }
}

fn main() -> anyhow::Result<()> {
    let arg_name: Vec<String> = env::args().collect();
    if arg_name.len() == 2 && arg_name[1].as_str() == "--run-child-process" {
        return main_child();
    }

    match Args::parse().command {
        Command::Run { workers, iterations, ports, batch_size, checkpoint_file, files} => {
            let workers = workers.or(Some(num_cpus::get())).unwrap();
            let batch_size = batch_size
                .or(Some((1f64 / ports as f64 / 100f64).ceil() as usize))
                .unwrap();

            main_server(
                workers,
                iterations,
                ports,
                files,
                batch_size,
                checkpoint_file,
                &*arg_name[0],
            )
        }
        
        Command::Checkpoint { command } => main_checkpoint_cli(command)
    }
}
