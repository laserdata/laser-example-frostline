use crate::handler::{Delivered, Handler};
use crate::latency::Latency;
use crate::sampler::Sampler;
use crate::{ConsumerError, ConsumerSummary};
use frostline_shared::codec::{Codec, Headers, SchemaSet};
use frostline_shared::measure::{Observed, Report, WindowAccumulator};
use frostline_shared::names::{Frame, Header};
use frostline_shared::policy::RolePolicy;
use frostline_shared::reports::ReportPublisher;
use std::collections::BTreeSet;
use std::time::Instant;
use tracing::info;

/// What one reader does with each record it receives, the same for filtered and full-feed reads.
pub struct Progress {
    pub codec: Codec,
    pub schemas: SchemaSet,
    pub policy: RolePolicy,
    pub handler: Box<dyn Handler>,
    pub accumulator: WindowAccumulator,
    pub reports: ReportPublisher,
    pub resume: crate::resume::ResumeState,
    pub sampler: Sampler,
    pub partitions: u32,
    pub finite: bool,
    pub summary: ConsumerSummary,
    pub closed: BTreeSet<u32>,
    pub latency: Latency,
    pub connection_opening_latency: Latency,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Business,
    Checkpoint,
}

impl Progress {
    /// Handle one record. A checkpoint publishes its receipt before this returns, so the caller acknowledges after it.
    pub async fn record(
        &mut self,
        partition: u32,
        offset: u64,
        payload: &[u8],
        frame: Frame,
    ) -> Result<Outcome, ConsumerError> {
        let event = self.codec.decode(payload, &self.schemas)?;
        if frame == Frame::Checkpoint {
            let checkpoint = event.checkpoint.ok_or(ConsumerError::Malformed(
                "a checkpoint frame without a checkpoint body",
            ))?;
            let repeated = self.accumulator.closed(partition, checkpoint.window_id);
            let receipt = self
                .accumulator
                .checkpoint(partition, checkpoint.window_id, offset)?;
            if !repeated {
                self.summary.add(&receipt);
            }
            self.reports.publish(&Report::Receipt(receipt)).await?;
            if checkpoint.last {
                self.closed.insert(partition);
            }
            return Ok(Outcome::Checkpoint);
        }
        let matched = self.policy.matches(&event);
        let closed = self.accumulator.closed(partition, event.window_id);
        let fresh = self.accumulator.record(Observed {
            partition_id: partition,
            window_id: event.window_id,
            event_id: event.event_id,
            sequence: event.sequence,
            payload,
            matched,
        })?;
        if !fresh && closed {
            self.summary.received_records += 1;
            self.summary.received_bytes += payload.len() as u64;
            self.summary.duplicates += 1;
        }
        if matched && fresh {
            let delivered = Delivered { partition, offset };
            self.handler.handle(&event, delivered);
            if self.sampler.allow(Instant::now())
                && let Some(line) = self.handler.narrate(&event, delivered)
            {
                info!(partition, offset, "{line}");
            }
        }
        Ok(Outcome::Business)
    }

    /// A finite read ends once every partition delivered its last checkpoint.
    pub fn finished(&self) -> bool {
        self.finite && self.closed.len() as u32 >= self.partitions
    }

    pub fn into_summary(mut self) -> ConsumerSummary {
        self.summary.status = self.handler.status();
        self.summary.fetch_latency = self.latency.summary();
        self.summary.connection_opening_latency = self.connection_opening_latency.summary();
        self.summary
    }
}

pub fn frame_of(headers: &Headers) -> Result<Frame, ConsumerError> {
    let value = headers
        .get(&Header::Frame.key())
        .ok_or(ConsumerError::Malformed(
            "a record without the frame header",
        ))?;
    let code = u8::try_from(value)
        .map_err(|_| ConsumerError::Malformed("a frame header that is not uint8"))?;
    Frame::from_code(code).ok_or(ConsumerError::Malformed("an unknown frame code"))
}
