#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

pub mod backpressure;
pub mod fleet;
pub mod incident;
pub mod pace;
pub mod partition;
pub mod publish;
pub mod windows;

use backpressure::Backpressure;
use fleet::Fleet;
use frostline_shared::codec::{CodecError, SchemaSet, load_schemas};
use frostline_shared::config::Mode;
use frostline_shared::domain::{LogicalTime, Sequence, WindowId};
use frostline_shared::event::{EventHeader, FleetEvent};
use frostline_shared::measure::Report;
use frostline_shared::names::{RunId, RunTopic};
use frostline_shared::reports::ReportPublisher;
use frostline_shared::runfile::RunFile;
use frostline_shared::{LaserFactory, Settings, ShutdownWatch, policy};
use laser_sdk::iggy::prelude::{Identifier, TopicClient};
use laser_sdk::prelude::{LaserError, ProducerMessage};
use pace::RateLimiter;
use publish::{Bounds, Outbound, PartitionPublisher};
use std::time::Duration;
use thiserror::Error;
use tokio::time::Instant;
use tracing::info;
use windows::WindowTracker;

const LOGICAL_STEP_MICROS: u64 = 1_000;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProducerSummary {
    pub windows: u64,
    pub records: u64,
    pub payload_bytes: u64,
    pub checkpoints: u64,
    pub requested_rate: u32,
    pub achieved_rate: f64,
    pub elapsed: Duration,
}

#[derive(Debug, Error)]
pub enum ProducerError {
    #[error(transparent)]
    Laser(#[from] LaserError),
    #[error(transparent)]
    Codec(#[from] CodecError),
    #[error("the publisher of partition {0} stopped")]
    PublisherStopped(u32),
    #[error("the publisher of partition {partition} failed: {source}")]
    PublisherFailed {
        partition: u32,
        source: std::sync::Arc<ProducerError>,
    },
    #[error("a publishing task failed: {0}")]
    Task(String),
    #[error("the reporter stopped while the producer waited for readers to catch up")]
    ReporterStopped,
    #[error(
        "run {0} already has published records. Start a new run instead of restarting its producer"
    )]
    AlreadyPublished(String),
}

/// Publish the fleet into the run until the finite count, the duration, or a shutdown signal.
/// With `backpressure`, the producer never runs further ahead of the readers than the reporter can hold.
pub async fn run(
    settings: &Settings,
    factory: &LaserFactory,
    run_file: &RunFile,
    mut shutdown: ShutdownWatch,
    mut backpressure: Option<Backpressure>,
) -> Result<ProducerSummary, ProducerError> {
    let run_id = &run_file.run_id;
    let laser = factory.connect(&run_id.stream()).await?;
    let topic = laser
        .client()
        .get_topic(
            &Identifier::named(&run_id.stream()).map_err(LaserError::from)?,
            &Identifier::named(&RunTopic::Changes.to_string()).map_err(LaserError::from)?,
        )
        .await
        .map_err(LaserError::from)?;
    if topic.is_some_and(|topic| topic.messages_count > 0) {
        return Err(ProducerError::AlreadyPublished(run_id.to_string()));
    }
    let schemas = load_schemas(&laser, settings.codec, &run_file.schema_ids).await?;
    let producer = laser
        .stream(run_id.stream())
        .topic(RunTopic::Changes.to_string())
        .producer()
        .create_stream(false)
        .create_topic(false)
        .batch_length(settings.batch_records)
        .build()
        .await?;
    let bounds = Bounds {
        records: settings.batch_records as usize,
        bytes: u64::from(settings.batch_bytes),
        queue: settings.publish_queue_records as usize,
        linger: settings.batch_linger,
    };
    let mut output = Output {
        publishers: (0..run_file.partitions)
            .map(|partition| PartitionPublisher::spawn(producer.clone(), partition, bounds))
            .collect(),
        reports: ReportPublisher::new(&laser, run_id).await?,
        tracker: WindowTracker::new(
            settings.checkpoint_records,
            run_file.partitions,
            policy::by_group(),
        ),
        envelope: Envelope::new(run_id.clone(), run_file.partitions),
        schemas,
        settings: settings.clone(),
        summary: ProducerSummary {
            requested_rate: settings.rate_per_second,
            ..ProducerSummary::default()
        },
    };
    let mut fleet = Fleet::new(settings);
    let mut limiter = RateLimiter::new(settings.rate_per_second, settings.rate_catch_up_records);
    let finite = settings.mode != Mode::Live;
    let started = Instant::now();
    let mut final_published = false;
    let deadline = settings.duration.map(|duration| started + duration);
    info!(run_id = %run_id, "producer started for run {run_id}");
    loop {
        if finite && output.summary.records >= settings.total_records {
            break;
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            break;
        }
        tokio::select! {
            () = shutdown.cancelled() => break,
            () = limiter.acquire() => {}
        }
        let step = fleet.next_step();
        let partition = step.partition;
        let header = output.envelope.header(partition, output.tracker.window());
        output
            .publish(partition, &step.into_event(header), false)
            .await?;
        if output.tracker.boundary_reached() {
            let last = finite && output.summary.records >= settings.total_records;
            output.close_window(last).await?;
            final_published |= last;
            if let Some(gate) = backpressure.as_mut() {
                tokio::select! {
                    () = shutdown.cancelled() => break,
                    admitted = gate.admit(output.summary.windows) => if !admitted {
                        return Err(ProducerError::ReporterStopped);
                    },
                }
            }
        }
    }
    if !output.tracker.is_empty() || (finite && !final_published) {
        output.close_window(true).await?;
    }
    let mut summary = output.finish().await?;
    summary.elapsed = started.elapsed();
    summary.achieved_rate =
        summary.records as f64 / summary.elapsed.as_secs_f64().max(f64::EPSILON);
    info!(
        run_id = %run_id,
        windows = summary.windows,
        records = summary.records,
        "producer for run {run_id} finished after {} windows and {} records",
        summary.windows,
        summary.records
    );
    Ok(summary)
}

struct Output {
    publishers: Vec<PartitionPublisher>,
    reports: ReportPublisher,
    tracker: WindowTracker,
    envelope: Envelope,
    schemas: SchemaSet,
    settings: Settings,
    summary: ProducerSummary,
}

impl Output {
    async fn publish(
        &mut self,
        partition: u32,
        event: &FleetEvent,
        checkpoint: bool,
    ) -> Result<(), ProducerError> {
        let codec = self.settings.codec;
        let payload = codec.encode(event, &self.schemas)?;
        let headers = codec.headers(event, &self.schemas)?;
        let payload_bytes = payload.len() as u64;
        if checkpoint {
            self.tracker.add_checkpoint(partition, payload_bytes);
            self.summary.checkpoints += 1;
        } else {
            self.tracker.observe(partition, event, &payload);
            self.summary.records += 1;
            self.summary.payload_bytes += payload_bytes;
        }
        let message = ProducerMessage::new(payload).with_headers(headers);
        self.publishers[partition as usize]
            .push(Outbound {
                message,
                payload_bytes,
            })
            .await
    }

    // Records are confirmed before the checkpoints, and checkpoints before the manifest.
    async fn close_window(&mut self, last: bool) -> Result<(), ProducerError> {
        for publisher in &self.publishers {
            publisher.flush().await?;
        }
        let window = self.tracker.window();
        for partition in 0..self.publishers.len() as u32 {
            let header = self.envelope.header(partition, window);
            let checkpoint = FleetEvent::checkpoint(header, partition, last);
            self.publish(partition, &checkpoint, true).await?;
        }
        for publisher in &self.publishers {
            publisher.flush().await?;
        }
        let manifest = self
            .tracker
            .close(&self.envelope.run_id, self.settings.codec);
        info!(
            window = %window,
            records = manifest.records(),
            bytes = manifest.payload_bytes(),
            "window {window} confirmed with {} records and {} payload bytes",
            manifest.records(),
            manifest.payload_bytes()
        );
        self.reports.publish(&Report::Manifest(manifest)).await?;
        self.summary.windows += 1;
        Ok(())
    }

    async fn finish(self) -> Result<ProducerSummary, ProducerError> {
        for publisher in self.publishers {
            publisher.close().await?;
        }
        Ok(self.summary)
    }
}

/// Envelope counters: a run-wide event id, a gapless sequence per partition, and the logical clock.
struct Envelope {
    run_id: RunId,
    next_event_id: u64,
    sequences: Vec<u64>,
}

impl Envelope {
    fn new(run_id: RunId, partitions: u32) -> Self {
        Self {
            run_id,
            next_event_id: 0,
            sequences: vec![0; partitions as usize],
        }
    }

    fn header(&mut self, partition: u32, window: WindowId) -> EventHeader {
        let event_id = self.next_event_id;
        self.next_event_id += 1;
        let sequence = &mut self.sequences[partition as usize];
        let header = EventHeader {
            run_id: self.run_id.clone(),
            event_id,
            window_id: window,
            sequence: Sequence(*sequence),
            logical_time: LogicalTime(event_id * LOGICAL_STEP_MICROS),
        };
        *sequence += 1;
        header
    }
}
