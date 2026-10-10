use super::{Session, while_progressing};
use crate::error::DemoError;
use crate::mixed;
use crate::report::{self, FinalReport, Reproduce};
use crate::walkthrough;
use frostline_producer::backpressure::Backpressure;
use frostline_shared::config::Catalog;
use frostline_shared::output::{act, phase};
use frostline_shared::{LaserFactory, ServiceHandle, Settings};
use tokio::time::interval;

/// One finite story: publish a fixed number of records, let every group finish, report, clean up.
pub async fn finite(
    settings: &Settings,
    factory: &LaserFactory,
    baselines: bool,
) -> Result<FinalReport, DemoError> {
    let control = ServiceHandle::new("readers");
    let watch = control.watch();
    let Session {
        laser,
        provisioned,
        reporter,
        mut readers,
    } = Session::start(settings, factory, baselines, &watch).await?;
    let mut reporter = Some(reporter);
    let result = async {
        let mut lines = Vec::new();
        let active = reporter
            .as_ref()
            .expect("the reporter runs until the story finishes");
        phase("publish");
        let backpressure = Backpressure::new(active.completed(), settings.max_pending_windows);
        let producing = frostline_producer::run(
            settings,
            factory,
            &provisioned.run_file,
            watch.clone(),
            Some(backpressure),
        );
        tokio::pin!(producing);
        let mut ticks = interval(settings.board_interval);
        let produced = loop {
            tokio::select! {
                produced = &mut producing => break produced?,
                _ = ticks.tick() => Session::collect_finished(&mut readers, &mut lines).await?,
            }
        };
        phase("drain");
        lines.extend(
            match while_progressing(
                Session::join_readers(&mut readers),
                active.completed(),
                settings.drain_timeout,
            )
            .await
            {
                Some(readers) => readers?,
                None => {
                    control.cancel();
                    return Err(DemoError::Incomplete(
                        "no window completed within the drain timeout".to_owned(),
                    ));
                }
            },
        );
        active
            .wait_for(produced.windows, settings.drain_timeout)
            .await?;
        phase("catalog");
        let previews = walkthrough::previews(&laser, &provisioned).await?;
        let revision_walkthrough = match settings.catalog {
            Catalog::Managed => walkthrough::pause_and_resume(&laser, &provisioned).await?,
            Catalog::Inline => Vec::new(),
        };
        phase("one log, many kinds of events");
        let (mixed_log, mixed_log_lines) = mixed::mixed_log(&laser, &provisioned).await?;
        let snapshot = reporter
            .take()
            .expect("the reporter is owned by this story")
            .stop()
            .await?;
        let final_report = FinalReport {
            run_id: provisioned.run_file.run_id.clone(),
            reproduce: Reproduce::of(settings, &settings.mode.to_string()),
            settings: settings.clone(),
            settings_digest: settings.digest(),
            codec: settings.codec.to_string(),
            catalog: settings.catalog.to_string(),
            producer: produced.into(),
            complete: snapshot.summary.windows == produced.windows && snapshot.expired == 0,
            summary: snapshot.summary,
            readers: lines,
            previews,
            revision_walkthrough,
            mixed_log,
            mixed_log_lines,
            expired_windows: snapshot.expired,
        };
        report::print_final(&final_report);
        report::write(&provisioned.directory, &final_report)?;
        act(&format!(
            "Wrote report.json and report.md to {}.",
            provisioned.directory.display()
        ));
        Ok::<_, DemoError>(final_report)
    }
    .await;
    let finished = Session::finish(
        settings,
        control,
        &laser,
        &provisioned.run_file,
        &mut readers,
        reporter.take(),
    )
    .await;
    if let Err(error) = &finished {
        tracing::warn!(run_id = %provisioned.run_file.run_id, "cleanup for run {} failed: {error}", provisioned.run_file.run_id);
    }
    let report = result?;
    finished?;
    Ok(report)
}
