use crate::ConsumerError;
use laser_sdk::iggy::prelude::{
    Consumer as NativeConsumer, ConsumerGroupClient, ConsumerOffsetClient, Identifier, IggyClient,
    MessageClient, PolledMessages, PollingStrategy,
};
use laser_sdk::prelude::Laser;
use std::collections::BTreeMap;
use std::sync::Arc;

/// One ordinary native poll per call, with explicit local offsets and no prefetch timing guess.
pub struct PollReader {
    client: Arc<IggyClient>,
    stream: Identifier,
    topic: Identifier,
    group: Identifier,
    consumer: NativeConsumer,
    offsets: BTreeMap<u32, u64>,
    cursor: usize,
    count: u32,
}

/// Whether a fresh reader resumes its checkpoint or explicitly replays the log.
pub enum PollStart {
    Stored,
    First,
}

impl PollReader {
    pub async fn new(
        laser: &Laser,
        stream: &str,
        topic: &str,
        group: &str,
        partitions: u32,
        count: u32,
        start: PollStart,
    ) -> Result<Self, ConsumerError> {
        laser
            .stream(stream)
            .topic(topic)
            .ensure_consumer_group(group)
            .await?;
        let stream = Identifier::named(stream)?;
        let topic = Identifier::named(topic)?;
        let group = Identifier::named(group)?;
        let client = laser.client();
        client.join_consumer_group(&stream, &topic, &group).await?;
        let consumer = NativeConsumer::group(group.clone());
        let mut offsets = BTreeMap::new();
        for partition in 0..partitions {
            let offset = match start {
                PollStart::First => 0,
                PollStart::Stored => client
                    .get_consumer_offset(&consumer, &stream, &topic, Some(partition))
                    .await?
                    .map_or(Ok(0), |stored| {
                        stored
                            .stored_offset
                            .checked_add(1)
                            .ok_or(ConsumerError::Malformed(
                                "a stored offset has no continuation",
                            ))
                    })?,
            };
            offsets.insert(partition, offset);
        }
        Ok(Self {
            client,
            stream,
            topic,
            consumer,
            group,
            offsets,
            cursor: 0,
            count,
        })
    }

    pub async fn poll(&mut self) -> Result<PolledMessages, ConsumerError> {
        if self.offsets.is_empty() {
            return Err(ConsumerError::Malformed("a poll reader without partitions"));
        }
        let (partition, offset) = self
            .offsets
            .iter()
            .nth(self.cursor % self.offsets.len())
            .map(|(partition, offset)| (*partition, *offset))
            .ok_or(ConsumerError::Malformed("a poll reader without partitions"))?;
        self.cursor = self.cursor.wrapping_add(1);
        let polled = self
            .client
            .poll_messages(
                &self.stream,
                &self.topic,
                Some(partition),
                &self.consumer,
                &PollingStrategy::offset(offset),
                self.count,
                false,
            )
            .await?;
        if let Some(last) = polled.messages.last() {
            self.offsets.insert(partition, last.header.offset + 1);
        }
        Ok(polled)
    }

    pub fn start_offset(&self, partition: u32) -> Option<u64> {
        self.offsets.get(&partition).copied()
    }

    pub async fn commit(&self, partition: u32, offset: u64) -> Result<(), ConsumerError> {
        self.client
            .store_consumer_offset(
                &self.consumer,
                &self.stream,
                &self.topic,
                Some(partition),
                offset,
            )
            .await?;
        Ok(())
    }

    pub fn retire(&mut self, partition: u32) {
        self.offsets.remove(&partition);
    }

    pub async fn close(&self) -> Result<(), ConsumerError> {
        self.client
            .leave_consumer_group(&self.stream, &self.topic, &self.group)
            .await?;
        Ok(())
    }
}
