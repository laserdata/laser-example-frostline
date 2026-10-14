#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

pub mod baseline;
pub mod handler;
pub mod hold;
pub mod latency;
pub mod maintenance;
pub mod poll;
pub mod progress;
pub mod reader;
pub mod regional;
mod resume;
pub mod safety;
pub mod sampler;

use frostline_shared::codec::{CodecError, load_schemas};
use frostline_shared::config::Mode;
use frostline_shared::measure::{Overflow, Receipt, WindowAccumulator};
use frostline_shared::names::{Role, RunTopic, SAFETY_CURRENT_GROUP};
use frostline_shared::policy::{self, RolePolicy};
use frostline_shared::reports::ReportPublisher;
use frostline_shared::runfile::{RunFile, RunFileError};
use frostline_shared::{LaserFactory, Settings, ShutdownWatch};
use handler::Handler;
use hold::Hold;
use laser_sdk::filters::FilteredStart;
use laser_sdk::iggy::prelude::IggyError;
use laser_sdk::prelude::LaserError;
use latency::{Latency, LatencySummary};
use progress::Progress;
use sampler::Sampler;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;
use tracing::info;

const BASELINE_SUFFIX: &str = "baseline";
/// The line a held reader prints once it finished reading and still holds its connections.
pub const HOLD_MARKER: &str = "frostline-reader-holding";
/// The line a reader prints last, followed by its summary as JSON.
pub const SUMMARY_MARKER: &str = "frostline-reader-summary";

/// Which reader to run: one group, filtered by the server or reading the full feed.
#[derive(Clone, Debug)]
pub struct ReaderSpec {
    pub group: String,
    pub baseline: bool,
    pub worker: u8,
    pub attempt: u32,
    pub replay: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ConsumerSummary {
    pub receipts: u64,
    pub matches: u64,
    pub received_records: u64,
    pub received_bytes: u64,
    pub duplicates: u64,
    pub fetch_latency: LatencySummary,
    #[serde(default)]
    pub connection_opening_latency: LatencySummary,
    pub status: String,
}

#[derive(Debug, Error)]
pub enum ConsumerError {
    #[error(transparent)]
    Laser(#[from] LaserError),
    #[error(transparent)]
    Iggy(#[from] IggyError),
    #[error(transparent)]
    Codec(#[from] CodecError),
    #[error(transparent)]
    RunFile(#[from] RunFileError),
    #[error(transparent)]
    Window(#[from] Overflow),
    #[error("the run has no policy for group {0}")]
    UnknownGroup(String),
    #[error("worker {worker} is outside this run's configured 1..={workers} workers per role")]
    InvalidWorker { worker: u8, workers: u8 },
    #[error("received {0}")]
    Malformed(&'static str),
}

impl ReaderSpec {
    pub fn filtered(group: impl Into<String>) -> Self {
        Self {
            group: group.into(),
            baseline: false,
            worker: 1,
            attempt: 1,
            replay: false,
        }
    }

    pub fn baseline(group: impl Into<String>) -> Self {
        Self {
            baseline: true,
            ..Self::filtered(group)
        }
    }
}

impl ConsumerSummary {
    pub fn add(&mut self, receipt: &Receipt) {
        self.receipts += 1;
        self.matches += receipt.matches;
        self.received_records += receipt.received_records;
        self.received_bytes += receipt.received_bytes;
        self.duplicates += receipt.duplicates;
    }
}

/// Read the run as one group until shutdown or, in a finite run, until every partition closed.
pub async fn run(
    settings: &Settings,
    factory: &LaserFactory,
    run_file: &RunFile,
    spec: ReaderSpec,
    shutdown: ShutdownWatch,
    hold: Option<Hold>,
) -> Result<ConsumerSummary, ConsumerError> {
    if spec.worker == 0
        || spec.worker > run_file.workers_per_role
        || settings.workers_per_role != run_file.workers_per_role
    {
        return Err(ConsumerError::InvalidWorker {
            worker: spec.worker,
            workers: run_file.workers_per_role,
        });
    }
    let run_id = &run_file.run_id;
    let policy = policy::by_group()
        .into_iter()
        .find(|(group, _)| *group == spec.group)
        .map(|(_, policy)| policy)
        .ok_or_else(|| ConsumerError::UnknownGroup(spec.group.clone()))?;
    let owned = owned_partitions(run_file.partitions, run_file.workers_per_role, &spec);
    if owned.is_empty() {
        return Ok(ConsumerSummary::default());
    }
    let laser = factory.connect(&run_id.stream()).await?;
    let schemas = load_schemas(&laser, run_file.codec, &run_file.schema_ids).await?;
    let bound = settings.fleet_size as usize;
    let mut progress = Progress {
        codec: run_file.codec,
        schemas,
        handler: handler_for(&spec.group, &policy, bound),
        accumulator: WindowAccumulator::new(
            run_id.clone(),
            spec.group.clone(),
            spec.baseline,
            spec.attempt,
        ),
        reports: ReportPublisher::new(&laser, run_id).await?,
        resume: resume::ResumeState::new(&laser, run_file, &spec, &owned),
        sampler: Sampler::new(settings.sampled_events_per_role_per_second),
        partitions: owned.len() as u32,
        finite: settings.mode != Mode::Live,
        summary: ConsumerSummary::default(),
        latency: Latency::default(),
        connection_opening_latency: Latency::default(),
        closed: BTreeSet::new(),
        policy,
    };
    let topic = RunTopic::Changes.to_string();
    info!(group = %spec.group, baseline = spec.baseline, "{} reader for group {} started", if spec.baseline { "full-feed" } else { "filtered" }, spec.group);
    if spec.baseline {
        let consumer = poll::PollReader::new(
            &laser,
            &run_id.stream(),
            &topic,
            &format!("{}-{BASELINE_SUFFIX}", spec.group),
            run_file.partitions,
            settings.poll_records,
            if spec.replay {
                poll::PollStart::First
            } else {
                poll::PollStart::Stored
            },
        )
        .await?;
        baseline::drive_baseline(
            consumer,
            &mut progress,
            settings.idle_interval,
            shutdown,
            hold,
        )
        .await?;
    } else {
        let binding = run_file
            .reader_group(&spec.group, spec.worker)?
            .binding
            .as_ref()
            .ok_or(ConsumerError::Malformed(
                "the run group has no configured policy",
            ))?;
        let group = laser
            .stream(run_id.stream())
            .topic(&topic)
            .consumer_group_id(binding.identity.group_id);
        let builder = group
            .reader()?
            .start(if spec.replay {
                FilteredStart::First
            } else {
                FilteredStart::Next
            })
            .max_unacked_pages(
                usize::try_from(run_file.checkpoint_records)
                    .unwrap_or(usize::MAX)
                    .saturating_add(1),
            )
            .count(settings.poll_records)
            .max_reply_bytes(settings.reply_bytes)
            .local_guard(settings.local_guard)
            .idle_interval(settings.idle_interval);
        let split = run_file.workers_per_role > 1;
        let builder = match split {
            true => owned
                .iter()
                .fold(builder, |builder, partition| builder.partition(*partition)),
            false => builder,
        };
        reader::drive(builder.build().await?, &mut progress, shutdown, hold).await?;
    }
    Ok(progress.into_summary())
}

/// Worker n of workers reads partitions where p % workers == n - 1. One worker reads them all.
pub fn owned_partitions(partitions: u32, workers: u8, spec: &ReaderSpec) -> Vec<u32> {
    let workers = u32::from(workers.max(1));
    if spec.baseline || workers == 1 {
        return (0..partitions).collect();
    }
    let index = u32::from(spec.worker.max(1)) - 1;
    (0..partitions)
        .filter(|partition| partition % workers == index)
        .collect()
}

fn handler_for(group: &str, policy: &RolePolicy, bound: usize) -> Box<dyn Handler> {
    match policy.role {
        Role::FoodSafety if group == SAFETY_CURRENT_GROUP => {
            Box::new(safety::SafetyHandler::new("Food safety current", bound))
        }
        Role::FoodSafety => Box::new(safety::SafetyHandler::new("Food safety", bound)),
        Role::Maintenance => Box::new(maintenance::MaintenanceHandler::new(bound)),
        Role::Regional => Box::new(regional::RegionalHandler::new(bound)),
    }
}

#[cfg(test)]
#[path = "reader_tests.rs"]
mod tests;
