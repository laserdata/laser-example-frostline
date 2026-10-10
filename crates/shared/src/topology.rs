use crate::names::{RunId, RunTopic};
use laser_sdk::iggy::prelude::{
    CompressionAlgorithm, Identifier, IggyExpiry, MaxTopicSize, StreamClient, TopicClient,
    TopicCreateOptions,
};
use laser_sdk::prelude::{Laser, LaserError};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

const REPORTS_PARTITIONS: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Retention {
    pub changes: Option<Duration>,
    pub reports: Option<Duration>,
}

/// The stream and topic incarnation a run measures. A recreated topic is a different source.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SourceIdentity {
    pub stream_id: u32,
    pub stream_created_at_micros: u64,
    pub topic_id: u32,
    pub topic_created_at_micros: u64,
}

#[derive(Debug, Error)]
pub enum TopologyError {
    #[error(transparent)]
    Laser(#[from] LaserError),
    #[error("topic {topic} was not found on stream {stream}")]
    Missing { stream: String, topic: String },
    #[error("topic {topic} has {actual} partitions, the run asked for {requested}")]
    Partitions {
        topic: String,
        requested: u32,
        actual: u32,
    },
    #[error("topic {topic} keeps messages for {actual:?}, the run asked for {requested:?}")]
    Retention {
        topic: String,
        requested: IggyExpiry,
        actual: IggyExpiry,
    },
}

/// Create the run stream and its two topics, or reuse them, and return the source incarnation.
pub async fn ensure_run(
    laser: &Laser,
    run: &RunId,
    partitions: u32,
    retention: Retention,
) -> Result<SourceIdentity, TopologyError> {
    let client = laser.client();
    let stream = run.stream();
    let stream_id = Identifier::named(&stream).map_err(LaserError::from)?;
    if client
        .get_stream(&stream_id)
        .await
        .map_err(LaserError::from)?
        .is_none()
    {
        client
            .create_stream(&stream)
            .await
            .map_err(LaserError::from)?;
    }
    for (topic, count, keep) in [
        (RunTopic::Changes, partitions, retention.changes),
        (RunTopic::Reports, REPORTS_PARTITIONS, retention.reports),
    ] {
        let name = topic.to_string();
        let topic_id = Identifier::named(&name).map_err(LaserError::from)?;
        if let Some(topic) = client
            .get_topic(&stream_id, &topic_id)
            .await
            .map_err(LaserError::from)?
        {
            if topic.partitions_count != count {
                return Err(TopologyError::Partitions {
                    topic: name,
                    requested: count,
                    actual: topic.partitions_count,
                });
            }
            continue;
        }
        let options = TopicCreateOptions {
            partitions_count: Some(count),
            compression_algorithm: Some(CompressionAlgorithm::default()),
            message_expiry: Some(expiry(keep)),
            max_topic_size: Some(MaxTopicSize::ServerDefault),
            ..TopicCreateOptions::default()
        };
        client
            .create_topic(&stream_id, &name, &options)
            .await
            .map_err(LaserError::from)?;
    }
    source_identity(laser, run).await
}

/// Read back the retention the server applied, since a requested value is not proof.
pub async fn verify_retention(
    laser: &Laser,
    run: &RunId,
    retention: Retention,
) -> Result<(), TopologyError> {
    let client = laser.client();
    let stream = run.stream();
    let stream_id = Identifier::named(&stream).map_err(LaserError::from)?;
    for (topic, keep) in [
        (RunTopic::Changes, retention.changes),
        (RunTopic::Reports, retention.reports),
    ] {
        let name = topic.to_string();
        let topic_id = Identifier::named(&name).map_err(LaserError::from)?;
        let details = client
            .get_topic(&stream_id, &topic_id)
            .await
            .map_err(LaserError::from)?
            .ok_or_else(|| TopologyError::Missing {
                stream: stream.clone(),
                topic: name.clone(),
            })?;
        let requested = expiry(keep);
        if details.message_expiry != requested {
            return Err(TopologyError::Retention {
                topic: name,
                requested,
                actual: details.message_expiry,
            });
        }
    }
    Ok(())
}

pub async fn source_identity(laser: &Laser, run: &RunId) -> Result<SourceIdentity, TopologyError> {
    let client = laser.client();
    let stream = run.stream();
    let topic = RunTopic::Changes.to_string();
    let stream_id = Identifier::named(&stream).map_err(LaserError::from)?;
    let topic_id = Identifier::named(&topic).map_err(LaserError::from)?;
    let missing = || TopologyError::Missing {
        stream: stream.clone(),
        topic: topic.clone(),
    };
    let stream_details = client
        .get_stream(&stream_id)
        .await
        .map_err(LaserError::from)?
        .ok_or_else(missing)?;
    let topic_details = client
        .get_topic(&stream_id, &topic_id)
        .await
        .map_err(LaserError::from)?
        .ok_or_else(missing)?;
    Ok(SourceIdentity {
        stream_id: stream_details.id,
        stream_created_at_micros: stream_details.created_at.as_micros(),
        topic_id: topic_details.id,
        topic_created_at_micros: topic_details.created_at.as_micros(),
    })
}

/// Delete the run stream. `false` when it was already gone.
pub async fn delete_run(laser: &Laser, run: &RunId) -> Result<bool, LaserError> {
    laser.stream(run.stream()).delete().await
}

fn expiry(keep: Option<Duration>) -> IggyExpiry {
    keep.map_or(IggyExpiry::NeverExpire, |duration| {
        IggyExpiry::ExpireDuration(duration.into())
    })
}
