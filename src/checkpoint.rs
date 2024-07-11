use rkyv::{Archive, Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Display;
use std::fs as stdfs;
use tokio::fs as tokfs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::io::{Read, Write};
use anyhow::bail;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

#[derive(Archive, Deserialize, Serialize, Debug)]
pub struct CheckpointEntry {
    pub name: String,

    /// map of {ports: {swaps: count}}
    pub frequency_map: HashMap<usize, HashMap<usize, usize>>,
}

#[derive(Archive, Deserialize, Serialize, Debug)]
pub struct Checkpoint {
    /// Version 1.

    pub time: u64,
    pub algorithms: HashMap<[u8; 32], CheckpointEntry>,
}

impl Checkpoint {
    // an u64 is very, very optimistic
    const VERSION: u64 = 1;

    pub fn read(path: String) -> anyhow::Result<Checkpoint> {
        let mut file = stdfs::File::open(&*path)?;
        let version = file.read_u64::<LittleEndian>()?;
        if version != Checkpoint::VERSION {
            bail!("version mismatch on checkpoint file {}: expected version {}, got {}", path, Checkpoint::VERSION, version)
        }
        let mut rest = Vec::new();
        if let Ok(metadata) = file.metadata() {
            rest.reserve(metadata.len() as usize);
        }
        file.read_to_end(&mut rest)?;

        Ok(unsafe { rkyv::archived_root::<Checkpoint>(&rest) }.deserialize(&mut rkyv::Infallible).unwrap())
    }

    pub async fn read_async(path: String) -> anyhow::Result<Checkpoint> {
        let mut file = tokfs::File::open(&*path).await?;
        let version = file.read_u64().await?;
        if version != Checkpoint::VERSION {
            bail!("version mismatch on checkpoint file {}: expected version {}, got {}", path, Checkpoint::VERSION, version)
        }
        let mut rest = Vec::new();
        if let Ok(metadata) = file.metadata().await {
            rest.reserve(metadata.len() as usize);
        }
        file.read_to_end(&mut rest).await?;

        Ok(unsafe { rkyv::archived_root::<Checkpoint>(&rest) }.deserialize(&mut rkyv::Infallible).unwrap())
    }

    pub fn write(&self, path: String) -> anyhow::Result<()> {
        let mut file = stdfs::OpenOptions::new().write(true).create(true).open(path)?;
        file.write_u64::<LittleEndian>(Checkpoint::VERSION)?;
        file.write_all(&*rkyv::to_bytes::<_, 256>(self)?)?;

        Ok(())
    }

    pub async fn write_async(&self, path: String) -> anyhow::Result<()> {
        let mut file = tokfs::OpenOptions::new().write(true).create(true).open(path).await?;
        file.write_u64(Checkpoint::VERSION).await?;
        file.write_all(&*rkyv::to_bytes::<_, 256>(self)?).await?;

        Ok(())
    }
}