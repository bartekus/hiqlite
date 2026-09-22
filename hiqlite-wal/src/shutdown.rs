use crate::error::Error;
use crate::{reader, writer};
use tokio::sync::oneshot;
use tracing::{debug, info};

/// A `ShutdownHandle` making it possible to do a graceful shutdown of the WAL logs tasks.
/// You must call `.shutdown()` for a clean shutdown.
#[derive(Debug, Clone)]
pub struct ShutdownHandle {
    tx_write: flume::Sender<writer::Action>,
    tx_read: flume::Sender<reader::Action>,
}

impl ShutdownHandle {
    pub(crate) fn new(
        tx_write: flume::Sender<writer::Action>,
        tx_read: flume::Sender<reader::Action>,
    ) -> Self {
        Self { tx_write, tx_read }
    }

    #[allow(unused)]
    pub async fn shutdown(&self) -> Result<(), Error> {
        let (tx_ack, ack) = oneshot::channel();
        debug!("Sending Action::Shutdown to WAL writer");
        self.tx_write
            .send_async(writer::Action::Shutdown(tx_ack))
            .await?;
        ack.await?;
        info!("WAL writer Shutdown complete");

        // The reader is deliberately **not** sent `Action::Shutdown`. This line used to be
        // `send_async(..)` without `.await`, which built the message and dropped it unsent.
        // Sending it for real would be worse: a reader that exits while the store still holds
        // senders strands any read queued behind the message, because a queued message lives
        // as long as any sender does (F-114, the same mechanism as the writer's). The reader
        // ends when its last sender is dropped, which is when nothing can ask it anything.
        let _ = &self.tx_read;

        Ok(())
    }
}
