use std::fs::{File, OpenOptions};
use std::io::Write;
use std::iter;
use crate::checkpoint::Checkpoint;
use clap::Subcommand;
use console::style;
use std::time::{Duration, SystemTime};
use itertools::Itertools;
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    Merge {
        file1: String,
        file2: String,

        #[arg(short = 'o')]
        output: String,
    },

    Csv {
        #[command(subcommand)]
        command: CsvCommand
    },

    InternalDump {
        file: String,
    },

    Summary {
        file: String,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum CsvCommand {
    /// Outputs a CSV file containing some averages for the given port count for each program
    Summary {
        file: String,

        #[arg(short = 'o')]
        output: String,

        #[arg(short = 'p')]
        ports: usize,
    },
    
    /// Outputs a CSV file containing the swap data for the given port count for the given program
    Dump {
        file: String,
        program_or_hash: String,

        #[arg(short = 'o')]
        output: String,

        #[arg(short = 'p')]
        ports: usize,
    },
    
    
    /// Outputs a CSV file containing some averages for the given program for each port count
    SwapDump {
        file: String,
        program_or_hash: String,

        #[arg(short = 'o')]
        output: String,
    }
}

pub fn main_checkpoint_cli(command: Command) -> anyhow::Result<()> {
    match command {
        Command::Merge {
            file1,
            file2,
            output,
        } => {
            let mut checkpoint1 = Checkpoint::read(file1)?;
            let checkpoint2 = Checkpoint::read(file2)?;

            for (alg_hash, entry2) in checkpoint2.algorithms {
                if let Some(entry1) = checkpoint1.algorithms.get_mut(&alg_hash) {
                    for (iterations, freq_map2) in entry2.frequency_map {
                        if let Some(freq_map1) = entry1.frequency_map.get_mut(&iterations) {
                            for (swaps, frequency) in freq_map2 {
                                *freq_map1.entry(swaps).or_insert(0) += frequency;
                            }
                        } else {
                            entry1.frequency_map.insert(iterations, freq_map2);
                        }
                    }
                } else {
                    checkpoint1.algorithms.insert(alg_hash, entry2);
                }
            }

            checkpoint1.time = SystemTime::now().elapsed()?.as_secs();

            checkpoint1.write(output)?;
        }

        Command::InternalDump { file } => println!("{:#?}", Checkpoint::read(file)?),

        Command::Summary { file } => {
            let checkpoint = Checkpoint::read(file.clone())?;
            let creation_time = (OffsetDateTime::UNIX_EPOCH + Duration::from_secs(checkpoint.time))
                .to_offset(UtcOffset::current_local_offset()?)
                .format(format_description!(
                    "[weekday repr:short] [day] [month repr:long] [year repr:full] [hour]:[minute]"
                ))?;

            println!(
                "{}",
                style(format!(
                    "Checkpoint file {file} created at {creation_time}:"
                ))
                .bold()
            );

            for (hash, alg) in checkpoint.algorithms {
                let iterations_computed = alg
                    .frequency_map
                    .iter()
                    .filter(|(_, items)| items.len() != 0)
                    .map(|(n, items)| {
                        let total_runs = items.values().fold(0, |acc, cur| acc + cur);
                        let average = items
                            .iter()
                            .fold(0f64, |acc, (swaps, freq)| acc + (swaps * freq) as f64)
                            / total_runs as f64;

                        let q1 = items.iter()
                            .sorted()
                            .map(|(swaps, freq)| iter::repeat(swaps).take(*freq))
                            .flatten()
                            .nth(items.len() / 4)
                            .unwrap();

                        let q3 = items.iter()
                            .sorted()
                            .map(|(swaps, freq)| iter::repeat(swaps).take(*freq))
                            .flatten()
                            .nth(((items.len() as f64) / (4f64/3f64)).ceil() as usize - 1)
                            .unwrap();

                        let iqr = q3 - q1;

                        format!("{n} ({total_runs} runs, mean {average:.2}, iqr {iqr})")
                    })
                    .collect::<Vec<String>>()
                    .join("\n                             ");

                println!(
                    "    ┗  {} {}:",
                    style(alg.name).bold().green(),
                    style(hex::encode(&hash[..4])).dim()
                );
                println!("       Port counts computed: {}", iterations_computed);
            }
        }

        Command::Csv { command } => match command {
            CsvCommand::Summary { file, output, ports } => {
                let checkpoint = Checkpoint::read(file)?;
                let mut output = OpenOptions::new().write(true).create_new(true).open(output)?;
                output.write_all("Program,Runs,Mean swaps,Lower quartile,Upper quartile,Interquartile range\n".as_ref())?;

                for (hash, alg) in checkpoint.algorithms {
                    if let Some(items) = alg.frequency_map.get(&ports) {
                        let total_runs = items.values().fold(0, |acc, cur| acc + cur);
                        let average = items
                            .iter()
                            .fold(0f64, |acc, (swaps, freq)| acc + (swaps * freq) as f64)
                            / total_runs as f64;
                        let q1 = items.iter()
                            .sorted()
                            .map(|(swaps, freq)| iter::repeat(swaps).take(*freq))
                            .flatten()
                            .nth(items.len() / 4)
                            .unwrap();
                        let q3 = items.iter()
                            .sorted()
                            .map(|(swaps, freq)| iter::repeat(swaps).take(*freq))
                            .flatten()
                            .nth(((items.len() as f64) / (4f64/3f64)).ceil() as usize - 1)
                            .unwrap();
                        let iqr = q3 - q1;

                        output.write_all(format!(
                            "{} ({}),{},{},{},{},{}\n",
                            alg.name,
                            hex::encode(&hash[..4]),
                            total_runs,
                            average,
                            q1,
                            q3,
                            iqr
                        ).as_bytes())?;
                    } else {
                        println!("Skipping {}", alg.name)
                    }
                }

                output.flush()?;
            }

            _ => unimplemented!()
        }
    }

    Ok(())
}
