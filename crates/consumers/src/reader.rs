use crate::ConsumerError;
use crate::hold::Hold;
use crate::progress::{Outcome, Progress, frame_of};
use frostline_shared::ShutdownWatch;
use laser_sdk::filters::FilteredReader;
use std::time::Instant;

/// Read filtered pages until shutdown or, in a finite run, until every partition closed.
pub async fn drive(
    mut reader: FilteredReader,
    progress: &mut Progress,
    mut shutdown: ShutdownWatch,
    hold: Option<Hold>,
) -> Result<(), ConsumerError> {
    let idle = reader.idle_interval();
    let result = async {
        progress.restore_finished().await?;
        while !progress.finished() {
            let connections = reader.data_connections_opened();
            let started = Instant::now();
            let (page, more) = tokio::select! {
                () = shutdown.cancelled() => break,
                page = reader.read_round() => page?,
            };
            let elapsed = started.elapsed();
            progress.latency.record(elapsed, reader.examined_in_round());
            if reader.data_connections_opened() > connections {
                progress
                    .connection_opening_latency
                    .record(elapsed, reader.examined_in_round());
            }
            let Some(page) = page else {
                if more {
                    continue;
                }
                tokio::select! {
                    () = shutdown.cancelled() => break,
                    () = tokio::time::sleep(idle) => continue,
                }
            };
            // A checkpoint is acknowledged with every record before it, only after its receipt is out.
            for record in &page.records {
                let headers = record.message.user_headers_map()?.unwrap_or_default();
                let outcome = progress
                    .record(
                        record.partition_id,
                        record.offset,
                        &record.message.payload,
                        frame_of(&headers)?,
                    )
                    .await?;
                if let Outcome::Checkpoint = outcome {
                    reader.ack_through(record).await?;
                }
            }
        }
        Ok::<(), ConsumerError>(())
    }
    .await;
    if result.is_ok()
        && let Some(hold) = hold
    {
        hold.wait(shutdown).await;
    }
    let closed = reader.close().await.map_err(ConsumerError::from);
    result.and(closed)
}
