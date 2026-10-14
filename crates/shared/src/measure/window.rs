use super::{Receipt, RollingDigest};
use crate::domain::{Sequence, WindowId};
use crate::names::RunId;
use std::collections::BTreeMap;
use thiserror::Error;

/// A reader's counters for the window each partition is in, closed by that partition's checkpoint.
#[derive(Debug)]
pub struct WindowAccumulator {
    run_id: RunId,
    group: String,
    baseline: bool,
    attempt: u32,
    partitions: BTreeMap<u32, Open>,
    closed: BTreeMap<u32, Receipt>,
}

#[derive(Debug, Default)]
struct Open {
    window: Option<WindowId>,
    last_sequence: Option<Sequence>,
    received_records: u64,
    received_bytes: u64,
    matches: u64,
    matched_bytes: u64,
    faults: u64,
    duplicates: u64,
    digest: RollingDigest,
}

#[derive(Debug, Error, PartialEq)]
pub enum Overflow {
    #[error(
        "checkpoint {checkpoint} on partition {partition_id} precedes the last completed window {completed}"
    )]
    RetiredCheckpoint {
        partition_id: u32,
        checkpoint: WindowId,
        completed: WindowId,
    },
    #[error("partition {partition_id} moved from window {open} to {next} without a checkpoint")]
    MissedCheckpoint {
        partition_id: u32,
        open: WindowId,
        next: WindowId,
    },
}

/// One business record as a reader saw it.
pub struct Observed<'a> {
    pub partition_id: u32,
    pub window_id: WindowId,
    pub event_id: u64,
    pub sequence: Sequence,
    pub payload: &'a [u8],
    pub matched: bool,
}

impl WindowAccumulator {
    pub fn new(run_id: RunId, group: impl Into<String>, baseline: bool, attempt: u32) -> Self {
        Self {
            run_id,
            group: group.into(),
            baseline,
            attempt,
            partitions: BTreeMap::new(),
            closed: BTreeMap::new(),
        }
    }

    /// Return false for a redelivery, which must not reach the handler again.
    pub fn record(&mut self, observed: Observed<'_>) -> Result<bool, Overflow> {
        if self
            .closed
            .get(&observed.partition_id)
            .is_some_and(|receipt| observed.window_id <= receipt.window_id)
        {
            return Ok(false);
        }
        let open = self.partitions.entry(observed.partition_id).or_default();
        if let Some(window) = open.window
            && window != observed.window_id
        {
            return Err(Overflow::MissedCheckpoint {
                partition_id: observed.partition_id,
                open: window,
                next: observed.window_id,
            });
        }
        open.window = Some(observed.window_id);
        // A redelivery crossed the wire, so its bytes count as received. It is never a second match.
        open.received_records += 1;
        open.received_bytes += observed.payload.len() as u64;
        if open
            .last_sequence
            .is_some_and(|last| observed.sequence <= last)
        {
            open.duplicates += 1;
            return Ok(false);
        }
        open.last_sequence = Some(observed.sequence);
        if observed.matched {
            open.matches += 1;
            open.matched_bytes += observed.payload.len() as u64;
            open.digest
                .push(observed.event_id, observed.sequence.0, observed.payload);
        }
        Ok(true)
    }

    pub fn closed(&self, partition: u32, window: WindowId) -> bool {
        self.closed
            .get(&partition)
            .is_some_and(|receipt| receipt.window_id >= window)
    }

    /// Close the partition's window at its checkpoint and return the receipt to publish.
    pub fn checkpoint(
        &mut self,
        partition_id: u32,
        window_id: WindowId,
        offset: u64,
    ) -> Result<Receipt, Overflow> {
        if let Some(receipt) = self.closed.get(&partition_id)
            && window_id < receipt.window_id
        {
            return Err(Overflow::RetiredCheckpoint {
                partition_id,
                checkpoint: window_id,
                completed: receipt.window_id,
            });
        }
        if let Some(receipt) = self
            .closed
            .get(&partition_id)
            .filter(|receipt| receipt.window_id == window_id)
        {
            return Ok(receipt.clone());
        }
        if let Some(open) = self
            .partitions
            .get(&partition_id)
            .and_then(|open| open.window)
            && open != window_id
        {
            return Err(Overflow::MissedCheckpoint {
                partition_id,
                open,
                next: window_id,
            });
        }
        let open = self.partitions.remove(&partition_id).unwrap_or_default();
        let last_sequence = open.last_sequence;
        self.partitions.insert(
            partition_id,
            Open {
                last_sequence,
                ..Open::default()
            },
        );
        let receipt = Receipt {
            run_id: self.run_id.clone(),
            group: self.group.clone(),
            baseline: self.baseline,
            partition_id,
            window_id,
            checkpoint_offset: offset,
            attempt: self.attempt,
            received_records: open.received_records,
            received_bytes: open.received_bytes,
            matches: open.matches,
            matched_bytes: open.matched_bytes,
            digest: open.digest.finish(),
            faults: open.faults,
            duplicates: open.duplicates,
        };
        self.closed.insert(partition_id, receipt.clone());
        Ok(receipt)
    }
}

#[cfg(test)]
#[path = "window_tests.rs"]
mod tests;
