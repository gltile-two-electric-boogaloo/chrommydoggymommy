use async_trait::async_trait;
use rkyv::ser::serializers::AllocSerializer;
use rkyv::{Archive, Deserialize, Infallible, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[async_trait]
pub(super) trait AsyncMessageSendExt {
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
pub(super) trait AsyncMessageRecvExt {
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
