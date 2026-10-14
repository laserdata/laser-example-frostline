use crate::ConsumerError;
use crate::poll::PollReader;
use crate::progress::{Progress, frame_of};
use frostline_shared::names::Frame;
use frostline_shared::names::RunTopic;
use frostline_shared::runfile::RunFile;
use laser_sdk::iggy::prelude::{
    Consumer, ConsumerOffsetClient, Identifier, MessageClient, PollingStrategy,
};
use laser_sdk::prelude::Laser;

pub struct ResumeState {
    laser: Laser,
    stream: String,
    group: String,
    owned: Vec<u32>,
    restore: bool,
}

impl ResumeState {
    pub fn new(laser: &Laser, run: &RunFile, spec: &crate::ReaderSpec, owned: &[u32]) -> Self {
        let group = if run.workers_per_role > 1 {
            format!("{}-worker-{}", spec.group, spec.worker)
        } else {
            spec.group.clone()
        };
        Self {
            laser: laser.clone(),
            stream: run.run_id.stream(),
            group,
            owned: owned.to_vec(),
            restore: !spec.replay,
        }
    }
}

impl Progress {
    /// Inspect each durable checkpoint before resuming after it. A final checkpoint ends a finite reader.
    pub async fn restore_finished(&mut self) -> Result<(), ConsumerError> {
        if !self.finite || !self.resume.restore {
            return Ok(());
        }
        let stream = Identifier::named(&self.resume.stream)?;
        let topic = Identifier::named(&RunTopic::Changes.to_string())?;
        let consumer = Consumer::group(Identifier::named(&self.resume.group)?);
        for partition in self.resume.owned.clone() {
            let stored = self
                .resume
                .laser
                .client()
                .get_consumer_offset(&consumer, &stream, &topic, Some(partition))
                .await?;
            if let Some(stored) = stored {
                self.inspect_checkpoint(partition, stored.stored_offset)
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn restore_finished_baseline(
        &mut self,
        reader: &mut PollReader,
    ) -> Result<(), ConsumerError> {
        if !self.finite || !self.resume.restore {
            return Ok(());
        }
        for partition in self.resume.owned.clone() {
            if let Some(offset) = reader
                .start_offset(partition)
                .and_then(|offset| offset.checked_sub(1))
            {
                self.inspect_checkpoint(partition, offset).await?;
                if self.closed.contains(&partition) {
                    reader.retire(partition);
                }
            }
        }
        Ok(())
    }

    async fn inspect_checkpoint(
        &mut self,
        partition: u32,
        offset: u64,
    ) -> Result<(), ConsumerError> {
        let messages = self
            .resume
            .laser
            .client()
            .poll_messages(
                &Identifier::named(&self.resume.stream)?,
                &Identifier::named(&RunTopic::Changes.to_string())?,
                Some(partition),
                &Consumer::new(Identifier::named("frostline-resume-check")?),
                &PollingStrategy::offset(offset),
                1,
                false,
            )
            .await?;
        if let Some(message) = messages
            .messages
            .first()
            .filter(|message| message.header.offset == offset)
        {
            let frame = frame_of(&message.user_headers_map()?.unwrap_or_default())?;
            if frame == Frame::Checkpoint
                && self
                    .codec
                    .decode(&message.payload, &self.schemas)?
                    .checkpoint
                    .is_some_and(|checkpoint| checkpoint.last)
            {
                self.closed.insert(partition);
            }
        }
        Ok(())
    }
}
