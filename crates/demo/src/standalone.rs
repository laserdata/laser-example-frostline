use crate::cleanup::cleanup;
use crate::doctor;
use crate::error::DemoError;
use crate::provision;
use crate::report::board_lines;
use crate::reporter::Snapshot;
use frostline_shared::measure::{Aggregator, Subscription};
use frostline_shared::output::{act, board, phase};
use frostline_shared::reports::ReportReader;
use frostline_shared::runfile::RunFile;
use frostline_shared::{LaserFactory, Settings};
use std::path::Path;
use std::time::Duration;

const REPORT_WAIT: Duration = Duration::from_secs(2);

/// Create a run for separate producer and consumer processes, and print where its run file is.
pub async fn setup(settings: &Settings, factory: &LaserFactory) -> Result<(), DemoError> {
    let doctor = doctor::run(factory).await?;
    doctor.require(settings)?;
    phase("setup");
    let laser = factory.connect("frostline-setup").await?;
    let provisioned = provision::provision(&laser, settings, factory.target().host).await?;
    act(&format!(
        "Start the producer and one consumer per group with --manifest {}.",
        provisioned.path.display()
    ));
    laser.close().await?;
    Ok(())
}

/// Read every report a run published so far and print its board.
pub async fn report(factory: &LaserFactory, manifest: &Path) -> Result<(), DemoError> {
    let run_file = RunFile::read(manifest)?;
    let laser = factory.connect(&run_file.run_id.stream()).await?;
    let subscriptions = run_file
        .groups
        .iter()
        .map(|record| Subscription {
            group: record.group.clone(),
            baseline: false,
        })
        .collect();
    let partitions = (0..run_file.partitions).collect();
    let mut aggregator = Aggregator::new(
        run_file.run_id.clone(),
        partitions,
        subscriptions,
        u32::MAX as usize,
    );
    let mut reader = ReportReader::new(
        &laser,
        &run_file.run_id,
        &format!("frostline-report-{}", run_file.run_id),
    )
    .await?;
    while let Some(report) = reader.next(REPORT_WAIT).await? {
        aggregator.apply(report)?;
    }
    reader.close().await?;
    phase("report");
    board(&board_lines(&Snapshot {
        summary: aggregator.totals().summary(),
        producer_window: aggregator.producer_window(),
        pending: aggregator.first_pending(),
        expired: aggregator.expired(),
    }));
    laser.close().await?;
    Ok(())
}

pub async fn cleanup_run(factory: &LaserFactory, manifest: &Path) -> Result<(), DemoError> {
    let run_file = RunFile::read(manifest)?;
    let laser = factory.connect(&run_file.run_id.stream()).await?;
    phase("cleanup");
    cleanup(&laser, &run_file).await?;
    if manifest.with_extension("lock").exists() {
        std::fs::remove_file(manifest.with_extension("lock")).map_err(|source| {
            frostline_shared::runfile::RunFileError::Io {
                path: manifest.with_extension("lock"),
                source,
            }
        })?;
    }
    laser.close().await?;
    Ok(())
}
