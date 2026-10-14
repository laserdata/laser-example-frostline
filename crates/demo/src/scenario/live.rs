use super::Session;
use crate::error::DemoError;
use crate::report::{self, FinalReport, Reproduce};
use frostline_producer::backpressure::Backpressure;
use frostline_shared::output::{act, board, phase};
use frostline_shared::{LaserFactory, ServiceHandle, Settings, shutdown_signal};
use tokio::time::{interval, timeout};

/// The living fleet: publish until Ctrl+C, print the board, then drain and report.
pub async fn live(settings: &Settings, factory: &LaserFactory) -> Result<FinalReport, DemoError> {
    let control = ServiceHandle::new("readers");
    let watch = control.watch();
    let ingress = ServiceHandle::new("fleet");
    let Session {
        laser,
        provisioned,
        reporter,
        mut readers,
    } = Session::start(settings, factory, false, &watch).await?;
    let mut reporter = Some(reporter);
    let result = async {
    let active = reporter.as_ref().expect("the reporter runs until the story finishes");
    phase("running until Ctrl+C");
    let backpressure = Backpressure::new(active.completed(), settings.max_pending_windows);
    let producer = tokio::spawn({
        let (settings, factory, run_file, watch) = (
            settings.clone(),
            factory.clone(),
            provisioned.run_file.clone(),
            ingress.watch(),
        );
        async move {
            frostline_producer::run(&settings, &factory, &run_file, watch, Some(backpressure)).await
        }
    });
    let mut ticks = interval(settings.board_interval);
    let signal = shutdown_signal();
    tokio::pin!(signal);
    // A reader that ends early failed, for example after a purge, so the story stops and reports it.
    loop {
        tokio::select! {
            () = &mut signal => break,
            _ = ticks.tick() => {
                board(&report::board_lines(&active.snapshot()));
                if producer.is_finished() || readers.iter().any(|(_, task)| task.is_finished()) {
                    break;
                }
            }
        }
    }
    phase("drain");
    ingress.cancel();
    let produced = producer
        .await
        .map_err(|error| DemoError::Incomplete(format!("the producer task failed: {error}")))??;
    let drained = active
        .wait_for(produced.windows, settings.drain_timeout)
        .await;
    control.cancel();
    let lines = timeout(settings.drain_timeout, Session::join_readers(&mut readers))
        .await
        .map_err(|_| {
            DemoError::Incomplete("the readers did not stop before the drain timeout".to_owned())
        })??;
    drained?;
    let snapshot = reporter.take().expect("the reporter is owned by this story").stop().await?;
    let final_report = FinalReport {
        run_id: provisioned.run_file.run_id.clone(),
        // The fleet ignores time and mode, and the totals cover a prefix of complete windows, so a finite run of that length gives the same totals.
        reproduce: Reproduce::of(
            &Settings {
                total_records: snapshot.summary.source_records,
                ..settings.clone()
            },
            "finite",
        ),
        settings: settings.clone(),
        settings_digest: settings.digest(),
        codec: settings.codec.to_string(),
        catalog: settings.catalog.to_string(),
        producer: produced.into(),
        complete: snapshot.summary.windows == produced.windows && snapshot.expired == 0,
        summary: snapshot.summary,
        readers: lines,
        previews: Vec::new(),
        revision_walkthrough: Vec::new(),
        mixed_log: Vec::new(),
        mixed_log_lines: Vec::new(),
        expired_windows: snapshot.expired,
    };
    report::print_final(&final_report);
    report::write(&provisioned.directory, &final_report)?;
    act(&format!(
        "Wrote report.json and report.md to {}.",
        provisioned.directory.display()
    ));
    Ok::<_, DemoError>(final_report)
    }.await;
    ingress.cancel();
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
