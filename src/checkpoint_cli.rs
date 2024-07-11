use std::time::SystemTime;
use clap::Subcommand;
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
        
        _ => unimplemented!()
    }

    Ok(())
}