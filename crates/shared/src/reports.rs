use crate::measure::Report;
use crate::names::{RunId, RunTopic};
use laser_sdk::iggy::prelude::{HeaderKey, HeaderValue};
use laser_sdk::prelude::{
    CommitPolicy, Consumer, ConsumerStart, ContentType, Laser, LaserError, Producer,
    ProducerMessage, Routing,
};
use laser_sdk::wire::headers::CONTENT_TYPE;
use std::str::FromStr;
use std::time::Duration;

const REPORTS_PARTITION: u32 = 0;

/// Publishes window manifests and receipts to the run's `reports` topic.
#[derive(Clone)]
pub struct ReportPublisher {
    producer: Producer,
}

impl ReportPublisher {
    pub async fn new(laser: &Laser, run: &RunId) -> Result<Self, LaserError> {
        let producer = laser
            .stream(run.stream())
            .topic(RunTopic::Reports.to_string())
            .producer()
            .create_stream(false)
            .create_topic(false)
            .batch_length(1)
            .build()
            .await?;
        Ok(Self { producer })
    }

    pub async fn publish(&self, report: &Report) -> Result<(), LaserError> {
        let payload =
            serde_json::to_vec(report).map_err(|error| LaserError::Codec(error.to_string()))?;
        let message = ProducerMessage::new(payload).header(
            HeaderKey::from_str(CONTENT_TYPE)?,
            HeaderValue::from(ContentType::Json.code()),
        );
        self.producer
            .send_with_routing(message, Routing::Partition(REPORTS_PARTITION))
            .await
            .map(|_| ())
    }
}

/// Reads every report of a run from the start, for the aggregator.
pub struct ReportReader {
    consumer: Consumer,
}

impl ReportReader {
    pub async fn new(laser: &Laser, run: &RunId, reader: &str) -> Result<Self, LaserError> {
        let consumer = laser
            .stream(run.stream())
            .topic(RunTopic::Reports.to_string())
            .consumer(reader, REPORTS_PARTITION)
            .start_at(ConsumerStart::First)
            .commit_policy(CommitPolicy::Disabled)
            .allow_replay()
            .build()
            .await?;
        Ok(Self { consumer })
    }

    /// The next report, or `None` when nothing arrived within `wait`.
    pub async fn next(&mut self, wait: Duration) -> Result<Option<Report>, LaserError> {
        match self.consumer.next_within(wait).await {
            Ok(message) => serde_json::from_slice(&message.payload)
                .map(Some)
                .map_err(|error| LaserError::Codec(error.to_string())),
            Err(LaserError::Timeout(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub async fn close(mut self) -> Result<(), LaserError> {
        self.consumer.shutdown().await
    }
}
