use crate::ConsumerError;
use crate::hold::Hold;
use crate::poll::PollReader;
use crate::progress::{Outcome, Progress, frame_of};
use frostline_shared::ShutdownWatch;
use std::time::{Duration, Instant};

/// Read ordinary native batches, apply the same domain handler, and commit checkpoints after receipts.
pub async fn drive_baseline(
    mut reader: PollReader,
    progress: &mut Progress,
    idle: Duration,
    mut shutdown: ShutdownWatch,
    hold: Option<Hold>,
) -> Result<(), ConsumerError> {
    let result = async {
        progress.restore_finished_baseline(&mut reader).await?;
        while !progress.finished() {
            let started = Instant::now();
            let fetched = tokio::select! { () = shutdown.cancelled() => break, result = reader.poll() => result? };
            progress.latency.record(started.elapsed(), u64::from(fetched.count));
            if fetched.messages.is_empty() {
                tokio::select! { () = shutdown.cancelled() => break, () = tokio::time::sleep(idle) => {} }
                continue;
            }
            for message in fetched.messages {
                let partition = fetched.partition_id;
                let frame = frame_of(&message.user_headers_map()?.unwrap_or_default())?;
                if let Outcome::Checkpoint = progress.record(partition, message.header.offset, &message.payload, frame).await? {
                    reader.commit(partition, message.header.offset).await?;
                    if progress.closed.contains(&partition) && progress.finite { reader.retire(partition); }
                }
            }
        }
        Ok::<(), ConsumerError>(())
    }.await;
    if result.is_ok()
        && let Some(hold) = hold
    {
        hold.wait(shutdown).await;
    }
    result.and(reader.close().await)
}
