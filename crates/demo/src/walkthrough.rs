use crate::error::DemoError;
use crate::provision::Provisioned;
use frostline_shared::names::{Role, RunTopic, SAFETY_CURRENT_GROUP};
use frostline_shared::output::act;
use laser_sdk::filters::FilterErrorReason;
use laser_sdk::prelude::{Laser, LaserError};
use std::time::Duration;
use tokio::time::timeout;

const PREVIEW_RECORDS: u32 = 10;
const READ_TIMEOUT: Duration = Duration::from_secs(15);

/// Preview the food safety filter on every partition. A preview stores nothing and joins no group.
pub async fn previews(laser: &Laser, provisioned: &Provisioned) -> Result<Vec<String>, DemoError> {
    let run_file = &provisioned.run_file;
    let topic = laser
        .stream(run_file.run_id.stream())
        .topic(RunTopic::Changes.to_string());
    let group = topic.consumer_group(Role::FoodSafety.group());
    let mut lines = Vec::new();
    for partition in 0..run_file.partitions {
        let preview = group
            .filter()
            .preview(partition)
            .await?
            .max_records(PREVIEW_RECORDS)
            .send()
            .await?;
        let line = format!(
            "Preview of partition {partition}: examined {}, matched {}, stopped at {}.",
            preview.examined, preview.matched, preview.stop
        );
        act(&line);
        lines.push(line);
    }
    Ok(lines)
}

/// Pause the A/B revision after one record reached the reader, finish that record, and resume.
pub async fn pause_and_resume(
    laser: &Laser,
    provisioned: &Provisioned,
) -> Result<Vec<String>, DemoError> {
    let run_file = &provisioned.run_file;
    let Some(_) = run_file.group(SAFETY_CURRENT_GROUP)?.binding.as_ref() else {
        return Ok(Vec::new());
    };
    let topic = laser
        .stream(run_file.run_id.stream())
        .topic(RunTopic::Changes.to_string());
    let group = topic.consumer_group(format!("{SAFETY_CURRENT_GROUP}-replay"));
    group
        .create()
        .filter(
            frostline_shared::policy::by_group()
                .into_iter()
                .find(|(name, _)| *name == SAFETY_CURRENT_GROUP)
                .expect("A/B policy")
                .1
                .filter(run_file.codec, &run_file.schema_ids),
        )
        .build()
        .await?;
    let policy = group.filter();
    let active = policy
        .get()
        .await?
        .ok_or_else(|| DemoError::Incomplete("replay group is unbound".to_owned()))?;
    let mut reader = group.reader()?.count(1).build().await?;
    let mut lines = Vec::new();
    let mut say = |line: String| {
        act(&line);
        lines.push(line);
    };
    let outcome = async {
        let first = timeout(READ_TIMEOUT, reader.next_record())
            .await
            .map_err(|_| LaserError::Timeout("the first record of the A/B revision"))??;
        policy.set_revision_enabled(active.revision, false).await?;
        reader.ack(&first).await?;
        say(format!(
            "Paused revision {}. The record at partition {} offset {} already reached the reader and was still acknowledged.",
            active.revision, first.partition_id, first.offset
        ));
        match reader.try_next_page().await {
            Err(error) if error.filter_reason() == Some(FilterErrorReason::RevisionDisabled) => {
                say("The next read was refused with revision_disabled.".to_owned());
            }
            Err(error) => return Err(DemoError::from(error)),
            Ok(_) => return Err(DemoError::Incomplete("a paused revision kept reading".to_owned())),
        }
        policy.set_revision_enabled(active.revision, true).await?;
        let next = timeout(READ_TIMEOUT, reader.next_record())
            .await
            .map_err(|_| LaserError::Timeout("a record after resume"))??;
        reader.ack(&next).await?;
        say(format!("Resumed revision {}. Reading continued at offset {}.", active.revision, next.offset));
        Ok::<(), DemoError>(())
    }
    .await;
    let resumed = policy
        .set_revision_enabled(active.revision, true)
        .await
        .map_err(DemoError::from);
    let closed = reader.close().await.map_err(DemoError::from);
    let released = crate::cleanup::retire_binding(laser, &active)
        .await
        .map_err(DemoError::from);
    outcome.and(resumed.map(|_| ())).and(closed).and(released)?;
    Ok(lines)
}
