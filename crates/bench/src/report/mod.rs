use crate::error::BenchError;
use crate::profiles::Profile;
use crate::runtime::BinaryVersion;
use crate::trial::Repetition;
use frostline_consumers::latency::LatencySummary;
use frostline_shared::measure::Reduction;
use serde::Serialize;
use std::fs;
use std::path::Path;

mod markdown;
mod stats;

use stats::{median, median_latency, readers, server_cpu, server_peak, wires};

pub(crate) const SERVER: &str = "iggy-server";
pub(crate) const SCOPE: &str = "Payload is the record payloads a reader was handed. TCP receive and send bytes are counted separately from successful socket syscalls under strace. The main reduction column uses received bytes, including records, headers, framing, checkpoints, login, and metadata. TCP/IP headers and retransmissions are excluded.";

/// A finished profile: the medians a reader needs, and every repetition behind them.
#[derive(Debug, Serialize)]
pub struct BenchResult {
    pub profile: &'static str,
    pub poll_records: u32,
    pub description: &'static str,
    pub reproduce: String,
    pub dataset: Vec<(&'static str, String)>,
    pub source_records: u64,
    pub source_bytes: u64,
    pub groups: Vec<GroupResult>,
    pub throughput: Throughput,
    pub binaries: Vec<BinaryVersion>,
    pub repetitions: Vec<Repetition>,
}

/// One group, filtered against the full feed. Payload is exact and equal in every repetition, wire and CPU are medians.
#[derive(Debug, Serialize)]
pub struct GroupResult {
    pub group: String,
    pub matches: u64,
    pub payload_bytes: u64,
    pub payload_full_feed_bytes: u64,
    pub payload_reduction: Reduction,
    pub wire_bytes: u64,
    pub wire_bytes_range: (u64, u64),
    pub wire_full_feed_bytes: u64,
    pub wire_reduction: Reduction,
    pub wire_sent_bytes: u64,
    pub wire_full_feed_sent_bytes: u64,
    pub connections: u32,
    pub full_feed_connections: u32,
    pub cpu_seconds: f64,
    pub full_feed_cpu_seconds: f64,
    pub peak_rss_bytes: u64,
    pub full_feed_peak_rss_bytes: u64,
    pub fetch_latency: LatencySummary,
    pub full_feed_fetch_latency: LatencySummary,
}

#[derive(Debug, Serialize)]
pub struct Throughput {
    pub publish_records_per_second: f64,
    pub filtered_wall_seconds: f64,
    pub full_feed_wall_seconds: f64,
    pub filtered_server_cpu_seconds: f64,
    pub full_feed_server_cpu_seconds: f64,
    /// Every filtered group makes the server examine the whole feed once.
    pub examined_records_per_server_cpu_second: f64,
    pub filtered_server_peak_rss_bytes: u64,
    pub full_feed_server_peak_rss_bytes: u64,
}

impl BenchResult {
    pub fn new(
        profile: &Profile,
        dataset: Vec<(&'static str, String)>,
        binaries: Vec<BinaryVersion>,
        repetitions: Vec<Repetition>,
    ) -> Result<Self, BenchError> {
        let first = repetitions
            .first()
            .ok_or_else(|| BenchError::Invalid("no repetition ran".to_owned()))?;
        if let Some(index) = repetitions
            .iter()
            .position(|repetition| repetition.summary != first.summary)
        {
            return Err(BenchError::Invalid(format!(
                "repetition {} selected other records than repetition 1",
                index + 1
            )));
        }
        let summary = &first.summary;
        let groups = summary
            .subscriptions
            .iter()
            .filter(|subscription| !subscription.baseline)
            .map(|subscription| {
                let filtered = readers(
                    &repetitions,
                    |repetition| &repetition.filtered,
                    &subscription.group,
                );
                let full = readers(
                    &repetitions,
                    |repetition| &repetition.full_feed,
                    &subscription.group,
                );
                let traced = wires(
                    &repetitions,
                    |repetition| &repetition.filtered_wire,
                    &subscription.group,
                );
                let traced_full = wires(
                    &repetitions,
                    |repetition| &repetition.full_feed_wire,
                    &subscription.group,
                );
                let wire: Vec<f64> = traced
                    .iter()
                    .map(|usage| usage.received_bytes as f64)
                    .collect();
                let wire_bytes = median(&wire) as u64;
                let wire_full_feed_bytes = median(
                    &traced_full
                        .iter()
                        .map(|usage| usage.received_bytes as f64)
                        .collect::<Vec<_>>(),
                ) as u64;
                GroupResult {
                    group: subscription.group.clone(),
                    matches: subscription.matches,
                    payload_bytes: subscription.received_bytes,
                    payload_full_feed_bytes: summary.source_bytes,
                    payload_reduction: subscription.reduction,
                    wire_bytes,
                    wire_bytes_range: (
                        wire.iter().copied().fold(f64::MAX, f64::min) as u64,
                        wire.iter().copied().fold(0.0, f64::max) as u64,
                    ),
                    wire_full_feed_bytes,
                    wire_reduction: Reduction::compute(wire_full_feed_bytes, wire_bytes),
                    wire_sent_bytes: median(
                        &traced
                            .iter()
                            .map(|usage| usage.sent_bytes as f64)
                            .collect::<Vec<_>>(),
                    ) as u64,
                    wire_full_feed_sent_bytes: median(
                        &traced_full
                            .iter()
                            .map(|usage| usage.sent_bytes as f64)
                            .collect::<Vec<_>>(),
                    ) as u64,
                    connections: median(
                        &traced
                            .iter()
                            .map(|usage| f64::from(usage.connections))
                            .collect::<Vec<_>>(),
                    ) as u32,
                    full_feed_connections: median(
                        &traced_full
                            .iter()
                            .map(|usage| f64::from(usage.connections))
                            .collect::<Vec<_>>(),
                    ) as u32,
                    cpu_seconds: median(
                        &filtered
                            .iter()
                            .map(|usage| usage.cpu_seconds)
                            .collect::<Vec<_>>(),
                    ),
                    full_feed_cpu_seconds: median(
                        &full
                            .iter()
                            .map(|usage| usage.cpu_seconds)
                            .collect::<Vec<_>>(),
                    ),
                    peak_rss_bytes: median(
                        &filtered
                            .iter()
                            .map(|usage| usage.peak_rss_bytes)
                            .collect::<Vec<_>>(),
                    ) as u64,
                    full_feed_peak_rss_bytes: median(
                        &full
                            .iter()
                            .map(|usage| usage.peak_rss_bytes)
                            .collect::<Vec<_>>(),
                    ) as u64,
                    fetch_latency: median_latency(
                        &repetitions,
                        |repetition| &repetition.filtered,
                        &subscription.group,
                    ),
                    full_feed_fetch_latency: median_latency(
                        &repetitions,
                        |repetition| &repetition.full_feed,
                        &subscription.group,
                    ),
                }
            })
            .collect::<Vec<_>>();
        let filtered_server_cpu_seconds =
            median(&server_cpu(&repetitions, |repetition| &repetition.filtered));
        let throughput = Throughput {
            publish_records_per_second: median(
                &repetitions
                    .iter()
                    .map(|repetition| repetition.publish.records_per_second)
                    .collect::<Vec<_>>(),
            ),
            filtered_wall_seconds: median(
                &repetitions
                    .iter()
                    .map(|repetition| repetition.filtered.wall_seconds)
                    .collect::<Vec<_>>(),
            ),
            full_feed_wall_seconds: median(
                &repetitions
                    .iter()
                    .map(|repetition| repetition.full_feed.wall_seconds)
                    .collect::<Vec<_>>(),
            ),
            filtered_server_cpu_seconds,
            full_feed_server_cpu_seconds: median(&server_cpu(&repetitions, |repetition| {
                &repetition.full_feed
            })),
            examined_records_per_server_cpu_second: (summary.source_records * groups.len() as u64)
                as f64
                / filtered_server_cpu_seconds.max(f64::EPSILON),
            filtered_server_peak_rss_bytes: median(&server_peak(&repetitions, |repetition| {
                &repetition.filtered
            })) as u64,
            full_feed_server_peak_rss_bytes: median(&server_peak(&repetitions, |repetition| {
                &repetition.full_feed
            })) as u64,
        };
        Ok(Self {
            profile: profile.name,
            poll_records: profile.poll_records,
            description: profile.description,
            reproduce: format!(
                "target/release/frostline-bench bench {} --poll-records {}",
                profile.name, profile.poll_records
            ),
            dataset,
            source_records: summary.source_records,
            source_bytes: summary.source_bytes,
            groups,
            throughput,
            binaries,
            repetitions,
        })
    }

    pub fn write(&self, directory: &Path) -> Result<(), BenchError> {
        fs::create_dir_all(directory)?;
        fs::write(
            directory.join("result.json"),
            serde_json::to_vec_pretty(self)?,
        )?;
        fs::write(directory.join("result.md"), self.markdown())?;
        Ok(())
    }
}
