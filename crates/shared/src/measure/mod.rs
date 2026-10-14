mod aggregate;
mod digest;
mod report;
mod window;

pub use aggregate::{AggregateError, Aggregator, Completion, WindowState};
pub use digest::{RollingDigest, hex};
pub use report::{ByteSize, Reduction, SubscriptionSummary, Summary, Totals};
pub use window::{Observed, Overflow, WindowAccumulator};

use crate::codec::Codec;
use crate::domain::WindowId;
use crate::names::RunId;
use serde::{Deserialize, Serialize};

/// What the producer confirmed for one window, and what each subscription must therefore receive.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WindowManifest {
    pub run_id: RunId,
    pub window_id: WindowId,
    pub codec: Codec,
    pub partitions: Vec<PartitionManifest>,
    pub expected: Vec<ExpectedMatch>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct PartitionManifest {
    pub partition_id: u32,
    pub records: u64,
    pub payload_bytes: u64,
    pub checkpoint_bytes: u64,
}

/// The records the hand-written predicate of one group selects in one partition of one window.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExpectedMatch {
    pub group: String,
    pub partition_id: u32,
    pub matches: u64,
    pub payload_bytes: u64,
    pub digest: String,
}

/// What one reader handled in one partition of one window, published when it reached the checkpoint.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Receipt {
    pub run_id: RunId,
    pub group: String,
    pub baseline: bool,
    pub partition_id: u32,
    pub window_id: WindowId,
    pub checkpoint_offset: u64,
    pub attempt: u32,
    pub received_records: u64,
    pub received_bytes: u64,
    pub matches: u64,
    pub matched_bytes: u64,
    pub digest: String,
    pub faults: u64,
    pub duplicates: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Report {
    Manifest(WindowManifest),
    Receipt(Receipt),
}

/// One reader subscription in a comparison: a group, and whether it reads the full feed.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Subscription {
    pub group: String,
    pub baseline: bool,
}

impl Receipt {
    pub fn subscription(&self) -> Subscription {
        Subscription {
            group: self.group.clone(),
            baseline: self.baseline,
        }
    }

    /// Redelivery counters can differ between attempts. The handled content must agree.
    pub fn same_content(&self, other: &Self) -> bool {
        self.run_id == other.run_id
            && self.group == other.group
            && self.baseline == other.baseline
            && self.partition_id == other.partition_id
            && self.window_id == other.window_id
            && self.checkpoint_offset == other.checkpoint_offset
            && self.matches == other.matches
            && self.matched_bytes == other.matched_bytes
            && self.digest == other.digest
    }
}

impl WindowManifest {
    pub fn payload_bytes(&self) -> u64 {
        self.partitions
            .iter()
            .map(|partition| partition.payload_bytes)
            .sum()
    }

    pub fn records(&self) -> u64 {
        self.partitions
            .iter()
            .map(|partition| partition.records)
            .sum()
    }

    pub fn expected(&self, group: &str, partition_id: u32) -> Option<&ExpectedMatch> {
        self.expected
            .iter()
            .find(|expected| expected.group == group && expected.partition_id == partition_id)
    }
}

#[cfg(test)]
mod tests;
