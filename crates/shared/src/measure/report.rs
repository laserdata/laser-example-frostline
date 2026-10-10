use super::WindowState;
use crate::domain::WindowId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
const STEP: f64 = 1000.0;

/// Running sums over the windows every subscription completed.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Totals {
    pub latest: Option<WindowId>,
    pub windows: u64,
    pub source_records: u64,
    pub source_bytes: u64,
    pub checkpoint_bytes: u64,
    pub subscriptions: BTreeMap<String, SubscriptionTotals>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct SubscriptionTotals {
    pub group: String,
    pub baseline: bool,
    pub received_records: u64,
    pub received_bytes: u64,
    pub matches: u64,
    pub matched_bytes: u64,
    pub duplicates: u64,
    pub faults: u64,
}

/// Payload avoided against reading the full feed. `NotApplicable` when the feed carried nothing.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", content = "fraction", rename_all = "snake_case")]
pub enum Reduction {
    Value(f64),
    NotApplicable,
}

/// A byte count shown in decimal units, from B up to PB, with its sign.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ByteSize(pub i128);

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Summary {
    pub windows: u64,
    pub latest: Option<WindowId>,
    pub source_records: u64,
    pub source_bytes: u64,
    pub checkpoint_bytes: u64,
    pub subscriptions: Vec<SubscriptionSummary>,
    /// Filtered subscriptions only: the sum of what they received against K times the feed.
    pub filtered_received_bytes: u64,
    pub filtered_baseline_bytes: u64,
    /// Payload the server kept back from the filtered subscriptions. Negative when redelivery outweighs filtering.
    pub filtered_saved_bytes: i64,
    pub filtered_reduction: Reduction,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SubscriptionSummary {
    pub group: String,
    pub baseline: bool,
    pub matches: u64,
    pub received_bytes: u64,
    pub saved_bytes: i64,
    pub reduction: Reduction,
    pub selectivity: Reduction,
    pub duplicates: u64,
}

impl Totals {
    pub fn fold(&mut self, window_id: WindowId, window: &WindowState) {
        let Some(manifest) = &window.manifest else {
            return;
        };
        self.latest = Some(window_id);
        self.windows += 1;
        self.source_records += manifest.records();
        self.source_bytes += manifest.payload_bytes();
        self.checkpoint_bytes += manifest
            .partitions
            .iter()
            .map(|partition| partition.checkpoint_bytes)
            .sum::<u64>();
        for ((subscription, _), receipt) in &window.receipts {
            let totals = self
                .subscriptions
                .entry(match subscription.baseline {
                    true => format!("{} baseline", subscription.group),
                    false => subscription.group.clone(),
                })
                .or_insert_with(|| SubscriptionTotals {
                    group: subscription.group.clone(),
                    baseline: subscription.baseline,
                    ..SubscriptionTotals::default()
                });
            totals.received_records += receipt.received_records;
            totals.received_bytes += receipt.received_bytes;
            totals.matches += receipt.matches;
            totals.matched_bytes += receipt.matched_bytes;
            totals.duplicates += receipt.duplicates;
            totals.faults += receipt.faults;
        }
    }

    pub fn summary(&self) -> Summary {
        let subscriptions: Vec<SubscriptionSummary> = self
            .subscriptions
            .values()
            .map(|totals| SubscriptionSummary {
                group: totals.group.clone(),
                baseline: totals.baseline,
                matches: totals.matches,
                received_bytes: totals.received_bytes,
                saved_bytes: saved(self.source_bytes, totals.received_bytes),
                reduction: Reduction::compute(self.source_bytes, totals.received_bytes),
                selectivity: Reduction::ratio(totals.matches, self.source_records),
                duplicates: totals.duplicates,
            })
            .collect();
        let filtered: Vec<&SubscriptionSummary> = subscriptions
            .iter()
            .filter(|summary| !summary.baseline)
            .collect();
        let filtered_received_bytes = filtered.iter().map(|summary| summary.received_bytes).sum();
        let filtered_baseline_bytes = self.source_bytes * filtered.len() as u64;
        Summary {
            windows: self.windows,
            latest: self.latest,
            source_records: self.source_records,
            source_bytes: self.source_bytes,
            checkpoint_bytes: self.checkpoint_bytes,
            filtered_saved_bytes: saved(filtered_baseline_bytes, filtered_received_bytes),
            filtered_reduction: Reduction::compute(
                filtered_baseline_bytes,
                filtered_received_bytes,
            ),
            filtered_received_bytes,
            filtered_baseline_bytes,
            subscriptions,
        }
    }
}

impl Reduction {
    /// `1 - delivered / baseline`. Redelivery can make it negative, and it is reported as is.
    pub fn compute(baseline: u64, delivered: u64) -> Self {
        if baseline == 0 {
            return Reduction::NotApplicable;
        }
        Reduction::Value(1.0 - delivered as f64 / baseline as f64)
    }

    pub fn ratio(part: u64, whole: u64) -> Self {
        if whole == 0 {
            return Reduction::NotApplicable;
        }
        Reduction::Value(part as f64 / whole as f64)
    }
}

impl fmt::Display for Reduction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reduction::Value(fraction) => write!(f, "{:.1}%", fraction * 100.0),
            Reduction::NotApplicable => f.write_str("not applicable"),
        }
    }
}

impl From<u64> for ByteSize {
    fn from(bytes: u64) -> Self {
        Self(i128::from(bytes))
    }
}

impl From<i64> for ByteSize {
    fn from(bytes: i64) -> Self {
        Self(i128::from(bytes))
    }
}

impl fmt::Display for ByteSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let mut value = self.0.unsigned_abs() as f64;
        let mut unit = 0;
        while value >= STEP && unit < UNITS.len() - 1 {
            value /= STEP;
            unit += 1;
        }
        match unit {
            0 => write!(f, "{sign}{value} {}", UNITS[0]),
            _ => write!(f, "{sign}{value:.2} {}", UNITS[unit]),
        }
    }
}

fn saved(baseline: u64, delivered: u64) -> i64 {
    let signed = |bytes: u64| i64::try_from(bytes).unwrap_or(i64::MAX);
    signed(baseline).saturating_sub(signed(delivered))
}
