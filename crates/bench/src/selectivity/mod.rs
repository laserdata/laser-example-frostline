use crate::collect::syscalls::WireUsage;
use crate::error::BenchError;
use crate::runtime::local_runtime;
use frostline_consumers::latency::LatencySummary;
use frostline_shared::LaserFactory;
use frostline_shared::measure::{ByteSize, Reduction};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::Path;
use std::process;
use std::time::Duration;

pub mod cases;
pub mod dataset;
mod oracle;
mod reader;
mod trial;

pub use reader::read;

pub(super) const BATCH_RECORDS: u64 = 1000;
pub(super) const TOPIC: &str = "selectivity";
pub(super) const MATCH_HEADER: &str = "bench.match";
pub(super) const MATCHED: u8 = 1;
// The last record of every dataset, so both readers know where the data ends.
pub(super) const SENTINEL: u8 = 2;
pub(super) const IDLE: Duration = Duration::from_millis(100);

/// What one reader received. The sentinel is not counted.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ReadOutcome {
    pub matched: u64,
    pub received_records: u64,
    pub received_payload_bytes: u64,
    pub fetch_latency: LatencySummary,
    #[serde(default)]
    pub fetch_and_ack_latency: LatencySummary,
}

/// One selectivity: the same dataset read with a header filter and as the full feed.
#[derive(Debug, Serialize)]
pub struct SelectivityRow {
    pub case: cases::Case,
    pub per_mille: u32,
    pub expected_matches: u64,
    pub filtered: ReadOutcome,
    pub full_feed: ReadOutcome,
    pub filtered_wire: WireUsage,
    pub full_feed_wire: WireUsage,
    pub wire_reduction: Reduction,
    pub filtered_server_cpu_seconds: Option<f64>,
    pub full_feed_server_cpu_seconds: Option<f64>,
    pub filtered_resources: trial::MeasuredRead,
    pub full_feed_resources: trial::MeasuredRead,
}

/// Measure untraced reads, then count socket bytes in separate traced passes.
pub async fn run(
    factory: &LaserFactory,
    directory: &Path,
    ticks_per_second: u64,
    matrix: bool,
) -> Result<Vec<SelectivityRow>, BenchError> {
    crate::profile::require(
        "/usr/bin/time",
        "Install GNU time to collect reader CPU and peak RSS.",
    )
    .await?;
    fs::create_dir_all(directory)?;
    let binary = env::current_exe()?;
    let runtime = local_runtime();
    if runtime.len() != 2 {
        return Err(BenchError::NoLocalRuntime);
    }
    let mut rows = Vec::new();
    let cases = if matrix {
        cases::MATRIX.as_slice()
    } else {
        cases::DENSITIES.as_slice()
    };
    for &case in cases {
        let per_mille = case.per_mille;
        let stream = format!(
            "frostline-bench-{}-{}",
            case.name.replace('_', "-"),
            process::id()
        );
        dataset::publish(factory, &stream, case).await?;
        let filtered_resources = trial::measure(
            &binary,
            &stream,
            true,
            directory,
            &runtime,
            ticks_per_second,
            case.name,
        )
        .await?;
        let full_feed_resources = trial::measure(
            &binary,
            &stream,
            false,
            directory,
            &runtime,
            ticks_per_second,
            case.name,
        )
        .await?;
        let filtered = filtered_resources.outcome;
        let full_feed = full_feed_resources.outcome;
        let filtered_wire =
            trial::trace(&binary, &stream, true, directory, &filtered, case.name).await?;
        let full_feed_wire =
            trial::trace(&binary, &stream, false, directory, &full_feed, case.name).await?;
        let server_cpu = |measurement: &trial::MeasuredRead| {
            measurement
                .server
                .iter()
                .find(|server| server.name == "iggy-server")
                .map(|server| server.cpu_seconds)
        };
        let expected_matches = case.matches();
        if filtered.matched != expected_matches
            || full_feed.matched != expected_matches
            || filtered.received_records != expected_matches
            || full_feed.received_records != case.records
            || filtered.received_payload_bytes != expected_matches * case.matched_bytes as u64
            || full_feed.received_payload_bytes != case.source_bytes()
        {
            return Err(BenchError::Invalid(format!(
                "at {per_mille} per mille the filter matched {} and the full feed {}, expected {expected_matches}",
                filtered.matched, full_feed.matched
            )));
        }
        let laser = factory.connect(&stream).await?;
        laser.stream(&stream).delete().await?;
        laser.close().await?;
        rows.push(SelectivityRow {
            case,
            per_mille,
            expected_matches,
            filtered,
            full_feed,
            wire_reduction: Reduction::compute(
                full_feed_wire.received_bytes,
                filtered_wire.received_bytes,
            ),
            filtered_wire,
            full_feed_wire,
            filtered_server_cpu_seconds: server_cpu(&filtered_resources),
            full_feed_server_cpu_seconds: server_cpu(&full_feed_resources),
            filtered_resources,
            full_feed_resources,
        });
    }
    fs::write(
        directory.join("selectivity.json"),
        serde_json::to_vec_pretty(&rows)?,
    )?;
    fs::write(directory.join("selectivity.md"), markdown(&rows))?;
    Ok(rows)
}

fn markdown(rows: &[SelectivityRow]) -> String {
    let mut text = String::from(
        "# Selectivity\n\nEach row records its exact dataset, predicate, and byte sizes in selectivity.json. Each dataset uses one partition. The full feed is an ordinary consumer. CPU, memory, and latency come from untraced reads. Socket bytes come from separate strace passes. They exclude TCP/IP headers and retransmissions.\n\n| case | matched | filtered wire | full feed wire | wire avoided | server CPU filtered | server CPU full feed | fetch p50 filtered | p99 | p99.9 | calls | examined per fetch | fetch p50 full feed | p99 | p99.9 | calls | records per fetch |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    let cpu = |seconds: Option<f64>| {
        seconds.map_or_else(
            || "not measured".to_owned(),
            |seconds| format!("{seconds:.2} s"),
        )
    };
    for row in rows {
        text.push_str(&format!(
            "| {} | {} | {} | {} | **{}** | {} | {} | {} us | {} us | {} us | {} | {} | {} us | {} us | {} us | {} | {} |\n",
            row.case.name,
            row.filtered.matched,
            ByteSize::from(row.filtered_wire.received_bytes),
            ByteSize::from(row.full_feed_wire.received_bytes),
            row.wire_reduction,
            cpu(row.filtered_server_cpu_seconds),
            cpu(row.full_feed_server_cpu_seconds),
            row.filtered.fetch_latency.p50_us,
            row.filtered.fetch_latency.p99_us,
            row.filtered.fetch_latency.p999_us,
            row.filtered.fetch_latency.fetches,
            row.filtered.fetch_latency.examined_per_fetch,
            row.full_feed.fetch_latency.p50_us,
            row.full_feed.fetch_latency.p99_us,
            row.full_feed.fetch_latency.p999_us,
            row.full_feed.fetch_latency.fetches,
            row.full_feed.fetch_latency.examined_per_fetch
        ));
    }
    text.push_str("\nThe complete cycle includes fetching, application selection, and acknowledgment on both paths. Empty filtered pages include the SDK's automatic acknowledgment.\n\n| case | cycle p50 filtered us | p99 | p99.9 | calls | cycle p50 raw us | p99 | p99.9 | calls |\n| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
    for row in rows {
        let (filtered, raw) = (
            row.filtered.fetch_and_ack_latency,
            row.full_feed.fetch_and_ack_latency,
        );
        text.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            row.case.name,
            filtered.p50_us,
            filtered.p99_us,
            filtered.p999_us,
            filtered.fetches,
            raw.p50_us,
            raw.p99_us,
            raw.p999_us,
            raw.fetches
        ));
    }
    text
}
