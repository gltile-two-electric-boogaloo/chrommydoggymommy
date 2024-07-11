use std::time::{Duration, SystemTime};
use clap::Subcommand;
use console::{style, Term};
use time::{Date, OffsetDateTime, PrimitiveDateTime, UtcOffset};
use time::macros::format_description;
use crate::checkpoint::Checkpoint;

#[derive(Debug, Subcommand, Clone)]
pub(crate) enum Command {
    Merge {
        file1: String,
        file2: String,

        #[arg(short = 'o')]
        output: String
    },

    Dump {
        file: String,

        #[arg(short = 'o')]
        output: String
    },

    InternalDump {
        file: String
    },

    Summary {
        file: String
    }
}

pub fn main_checkpoint_cli(command: Command) -> anyhow::Result<()> {
    match command {
        Command::Merge { file1, file2, output } => {
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
                            entry1.frequency_map.insert(iterations,  freq_map2);
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
                .format(format_description!("[weekday repr:short] [day] [month repr:long] [year repr:full] [hour]:[minute]"))?;

            println!("{}", style(format!("Checkpoint file {file} created at {creation_time}:")).bold());

            for (hash, alg) in checkpoint.algorithms {
                let iterations_computed = alg.frequency_map
                    .iter()
                    .map(|(n, items)| {
                        let total_runs = items.values()
                            .fold(0, |acc, cur| acc + cur);
                        let average = items.iter()
                            .fold(0f64, |acc, (swaps, freq)| acc + (swaps * freq) as f64)
                            / total_runs as f64;
                        
                        format!("{n} ({total_runs} runs, avg. swaps {average:.3})")
                    })
                    .collect::<Vec<String>>()
                    .join(", ");
                
                println!("    ┗  {} {}:", style(alg.name).bold().green(), style(hex::encode(&hash)[..8].to_string()).dim());
                println!("       Port counts computed: {}", iterations_computed);
            }
        }

        _ => unimplemented!()
    }

    Ok(())
}