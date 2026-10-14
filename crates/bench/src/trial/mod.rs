use crate::readers::ReaderLaunch;
mod read;

use crate::collect::ProcessUsage;
use crate::collect::phase::PhaseMonitor;
use crate::collect::process::CgroupMemory;
use crate::collect::syscalls::WireUsage;
use crate::error::BenchError;
use crate::profiles::Profile;
use crate::runtime::RuntimeProcess;
use frostline_consumers::ConsumerSummary;
use frostline_consumers::latency::LatencySummary;
use frostline_demo::cleanup::cleanup;
use frostline_demo::provision::provision;
use frostline_demo::reporter::{Reporter, Snapshot};
use frostline_shared::measure::{Subscription, Summary};
use frostline_shared::names::RunId;
use frostline_shared::output::phase;
use frostline_shared::policy;
use frostline_shared::runfile::RunFile;
use frostline_shared::{LaserFactory, ServiceHandle, Settings};
use serde::Serialize;
use std::path::PathBuf;
use tokio::time::Instant;

/// What every repetition of one profile shares.
pub struct TrialContext<'a> {
    pub profile: &'a Profile,
    pub factory: &'a LaserFactory,
    pub directory: PathBuf,
    pub consumer_binary: PathBuf,
    pub runtime: Vec<RuntimeProcess>,
    pub ticks_per_second: u64,
}

/// One fresh run of the profile: publish, read with filters, read the full feed, check against the manifests.
#[derive(Debug, Serialize)]
pub struct Repetition {
    pub run_id: RunId,
    pub filtered_first: bool,
    pub publish: PublishPhase,
    pub filtered: ReadPhase,
    pub full_feed: ReadPhase,
    pub filtered_wire: Vec<ReaderWire>,
    pub full_feed_wire: Vec<ReaderWire>,
    pub summary: Summary,
}

/// Bytes one reader moved through TCP in a traced pass. Tracing slows the reader, so this pass measures bytes only.
#[derive(Debug, Serialize)]
pub struct ReaderWire {
    pub group: String,
    pub baseline: bool,
    pub wire: WireUsage,
    pub delivery: ConsumerSummary,
}

#[derive(Debug, Serialize)]
pub struct PublishPhase {
    pub records: u64,
    pub payload_bytes: u64,
    pub windows: u64,
    pub wall_seconds: f64,
    pub records_per_second: f64,
    pub server: Vec<ServerUsage>,
}

#[derive(Debug, Serialize)]
pub struct ReadPhase {
    pub wall_seconds: f64,
    pub readers: Vec<ReaderUsage>,
    pub server: Vec<ServerUsage>,
}

#[derive(Debug, Serialize)]
pub struct ReaderUsage {
    pub group: String,
    pub baseline: bool,
    pub wall_seconds: f64,
    pub usage: ProcessUsage,
    /// One bounded filtered reader round, or one ordinary native batch poll.
    pub fetch_latency: LatencySummary,
    pub delivery: ConsumerSummary,
}

/// Phase CPU, sampled phase peaks, and the separately labelled lifetime high-water mark.
#[derive(Debug, Serialize)]
pub struct ServerUsage {
    pub name: String,
    pub cpu_seconds: f64,
    pub rss_bytes: u64,
    pub peak_rss_bytes: u64,
    pub lifetime_peak_rss_bytes: u64,
    pub peak_pss_bytes: u64,
    pub memory_samples: u64,
    pub memory_sample_interval_ms: u64,
    /// The runtime's cgroup, which a pressure run caps with `MemoryMax`.
    pub cgroup: Option<CgroupMemory>,
}

pub async fn repetition(context: &TrialContext<'_>, index: u8) -> Result<Repetition, BenchError> {
    let settings = context
        .profile
        .settings(context.directory.join(format!("repetition-{index}")));
    phase(&format!(
        "{} repetition {index} of {}",
        context.profile.name, context.profile.repetitions
    ));
    let laser = context.factory.connect("frostline-bench").await?;
    let provisioned = provision(&laser, &settings, context.factory.target().host).await?;
    let run_file = &provisioned.run_file;
    let groups: Vec<&'static str> = policy::by_group()
        .into_iter()
        .map(|(group, _)| group)
        .collect();
    let subscriptions = groups
        .iter()
        .flat_map(|group| {
            [false, true].map(|baseline| Subscription {
                group: (*group).to_owned(),
                baseline,
            })
        })
        .collect();
    let reporter = Reporter::spawn(
        &laser,
        &run_file.run_id,
        settings.partitions,
        subscriptions,
        settings.max_pending_windows,
    )
    .await?;
    let log_directory = settings
        .output_directory
        .clone()
        .unwrap_or_default()
        .join("logs");
    let variables = read::reader_variables(&settings);
    let launch = ReaderLaunch {
        binary: &context.consumer_binary,
        run_file: &provisioned.path,
        variables: &variables,
        log_directory: &log_directory,
    };
    phase("publish");
    let publish = publish(context, &settings, run_file).await?;
    let filtered_first = index % 2 == 1;
    let (filtered, full_feed) = if filtered_first {
        phase("read with filters");
        let filtered = read::read(context, &settings, &launch, &groups, false).await?;
        phase("read the full feed");
        let full_feed = read::read(context, &settings, &launch, &groups, true).await?;
        (filtered, full_feed)
    } else {
        phase("read the full feed");
        let full_feed = read::read(context, &settings, &launch, &groups, true).await?;
        phase("read with filters");
        let filtered = read::read(context, &settings, &launch, &groups, false).await?;
        (filtered, full_feed)
    };
    phase("count wire bytes with filters");
    let filtered_wire = read::trace(&settings, &launch, &groups, false, &filtered).await?;
    phase("count wire bytes of the full feed");
    let full_feed_wire = read::trace(&settings, &launch, &groups, true, &full_feed).await?;
    reporter
        .wait_for(publish.windows, settings.drain_timeout)
        .await?;
    let snapshot = reporter.stop().await?;
    validate(&snapshot, &publish)?;
    cleanup(&laser, run_file).await?;
    laser.close().await?;
    Ok(Repetition {
        run_id: run_file.run_id.clone(),
        filtered_first,
        publish,
        filtered,
        full_feed,
        filtered_wire,
        full_feed_wire,
        summary: snapshot.summary,
    })
}

async fn publish(
    context: &TrialContext<'_>,
    settings: &Settings,
    run_file: &RunFile,
) -> Result<PublishPhase, BenchError> {
    let monitor = PhaseMonitor::start(
        &context.runtime,
        context.ticks_per_second,
        &settings
            .output_directory
            .as_ref()
            .expect("the trial has an output directory")
            .join("publish-memory.jsonl"),
    )?;
    let started = Instant::now();
    let service = ServiceHandle::new("bench");
    let produced =
        frostline_producer::run(settings, context.factory, run_file, service.watch(), None).await?;
    let wall_seconds = started.elapsed().as_secs_f64();
    Ok(PublishPhase {
        records: produced.records,
        payload_bytes: produced.payload_bytes,
        windows: produced.windows,
        wall_seconds,
        records_per_second: produced.records as f64 / wall_seconds.max(f64::EPSILON),
        server: monitor.finish().await?,
    })
}

// A figure counts only when every window completed with matching digests and every full-feed reader got the whole feed.
fn validate(snapshot: &Snapshot, publish: &PublishPhase) -> Result<(), BenchError> {
    let summary = &snapshot.summary;
    if summary.windows != publish.windows || snapshot.expired != 0 {
        return Err(BenchError::Invalid(format!(
            "{} of {} windows completed, {} expired",
            summary.windows, publish.windows, snapshot.expired
        )));
    }
    if summary.source_bytes != publish.payload_bytes {
        return Err(BenchError::Invalid(format!(
            "manifests hold {} bytes, the producer published {}",
            summary.source_bytes, publish.payload_bytes
        )));
    }
    for subscription in summary
        .subscriptions
        .iter()
        .filter(|subscription| subscription.baseline)
    {
        if subscription.received_bytes != summary.source_bytes {
            return Err(BenchError::Invalid(format!(
                "the full-feed reader of {} received {} of {} bytes",
                subscription.group, subscription.received_bytes, summary.source_bytes
            )));
        }
    }
    Ok(())
}
