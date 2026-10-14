use crate::ProducerError;
use laser_sdk::prelude::{Producer, ProducerMessage, Routing};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout_at};
use tracing::warn;

/// Keeps one partition's records in order and batches them within record and byte bounds.
pub struct PartitionPublisher {
    partition: u32,
    failure: watch::Receiver<Option<Arc<ProducerError>>>,
    sender: mpsc::Sender<Command>,
    task: JoinHandle<Result<(), ProducerError>>,
}

pub struct Outbound {
    pub message: ProducerMessage,
    pub payload_bytes: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub records: usize,
    pub bytes: u64,
    pub queue: usize,
    pub linger: Duration,
}

enum Command {
    Record(Outbound),
    Flush(oneshot::Sender<()>),
}

impl PartitionPublisher {
    pub fn spawn(producer: Producer, partition: u32, bounds: Bounds) -> Self {
        let (sender, receiver) = mpsc::channel(bounds.queue.max(1));
        let (cause, failure) = watch::channel(None);
        let task = tokio::spawn(async move {
            match drain(producer, partition, bounds, receiver).await {
                Ok(()) => Ok(()),
                Err(error) => {
                    let source = Arc::new(error);
                    let _ = cause.send(Some(source.clone()));
                    Err(ProducerError::PublisherFailed { partition, source })
                }
            }
        });
        Self {
            partition,
            failure,
            sender,
            task,
        }
    }

    /// Queue one record. Waits while the partition queue is full, so memory stays bounded.
    pub async fn push(&self, outbound: Outbound) -> Result<(), ProducerError> {
        if self.sender.send(Command::Record(outbound)).await.is_err() {
            return Err(self.failure().await);
        }
        Ok(())
    }

    /// Wait until every queued record is confirmed by the server.
    pub async fn flush(&self) -> Result<(), ProducerError> {
        let (reply, confirmed) = oneshot::channel();
        if self.sender.send(Command::Flush(reply)).await.is_err() || confirmed.await.is_err() {
            return Err(self.failure().await);
        }
        Ok(())
    }

    async fn failure(&self) -> ProducerError {
        let mut failure = self.failure.clone();
        let source = failure
            .wait_for(|cause| cause.is_some())
            .await
            .ok()
            .and_then(|cause| cause.clone());
        source.map_or(ProducerError::PublisherStopped(self.partition), |source| {
            ProducerError::PublisherFailed {
                partition: self.partition,
                source,
            }
        })
    }

    pub async fn close(self) -> Result<(), ProducerError> {
        drop(self.sender);
        self.task
            .await
            .map_err(|error| ProducerError::Task(error.to_string()))?
    }
}

// A batch closes when it is full, when a flush arrives, or when the linger since its first record ends.
async fn drain(
    producer: Producer,
    partition: u32,
    bounds: Bounds,
    mut receiver: mpsc::Receiver<Command>,
) -> Result<(), ProducerError> {
    let mut batch: Vec<Outbound> = Vec::with_capacity(bounds.records);
    while let Some(first) = receiver.recv().await {
        let deadline = Instant::now() + bounds.linger;
        let mut batch_bytes = 0;
        let mut command = Some(first);
        let mut flush = None;
        while let Some(next) = command.take() {
            match next {
                Command::Record(outbound) => {
                    batch_bytes += outbound.payload_bytes;
                    batch.push(outbound);
                }
                Command::Flush(reply) => {
                    flush = Some(reply);
                    break;
                }
            }
            if batch.len() >= bounds.records || batch_bytes >= bounds.bytes {
                break;
            }
            command = timeout_at(deadline, receiver.recv()).await.ok().flatten();
        }
        send(&producer, partition, &mut batch).await?;
        if let Some(reply) = flush {
            let _ = reply.send(());
        }
    }
    Ok(())
}

async fn send(
    producer: &Producer,
    partition: u32,
    batch: &mut Vec<Outbound>,
) -> Result<(), ProducerError> {
    if batch.is_empty() {
        return Ok(());
    }
    let messages = batch.drain(..).map(|outbound| outbound.message);
    if let Err(error) = producer
        .send_batch_with_routing(messages, Some(Routing::Partition(partition)))
        .await
    {
        warn!(
            partition,
            "publishing to partition {partition} failed. {error}"
        );
        return Err(ProducerError::Laser(error));
    }
    Ok(())
}
