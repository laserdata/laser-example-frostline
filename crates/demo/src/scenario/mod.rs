mod codecs;
mod finite;
mod live;

pub use codecs::codecs;
pub use finite::finite;
pub use live::live;

use crate::cleanup::cleanup;
use crate::doctor::{self, Doctor};
use crate::error::DemoError;
use crate::provision::{self, Provisioned};
use crate::report;
use crate::reporter::Reporter;
use frostline_consumers::{ConsumerError, ConsumerSummary, ReaderSpec};
use frostline_shared::measure::Subscription;
use frostline_shared::names::SAFETY_CURRENT_GROUP;
use frostline_shared::output::phase;
use frostline_shared::policy;
use frostline_shared::runfile::RunFile;
use frostline_shared::{LaserFactory, ServiceHandle, Settings, ShutdownWatch};
use laser_sdk::prelude::Laser;
use std::collections::BTreeSet;
use std::future::Future;
use std::time::Duration;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::timeout;

pub type ReaderTask = (
    ReaderSpec,
    JoinHandle<Result<ConsumerSummary, ConsumerError>>,
);

/// A provisioned run with its connection, its reporter, and the readers running against it.
pub struct Session {
    pub laser: Laser,
    pub provisioned: Provisioned,
    pub reporter: Reporter,
    pub readers: Vec<ReaderTask>,
}

impl Session {
    /// Check the server, create the run, start the reporter and one reader per group.
    pub async fn start(
        settings: &Settings,
        factory: &LaserFactory,
        baselines: bool,
        shutdown: &ShutdownWatch,
    ) -> Result<Self, DemoError> {
        let doctor: Doctor = doctor::run(factory).await?;
        doctor.require(settings)?;
        report::run_profile(settings, &doctor);
        phase("setup");
        let host = factory.target().host;
        let probe = factory.connect("frostline-setup").await?;
        let provisioned = provision::provision(&probe, settings, host).await?;
        let laser = factory
            .connect(&provisioned.run_file.run_id.stream())
            .await?;
        probe.close().await?;
        let workers = settings.workers_per_role.max(1);
        let specs: Vec<ReaderSpec> = policy::by_group()
            .into_iter()
            .flat_map(|(group, _)| {
                let filtered = (1..=workers).map(move |worker| ReaderSpec {
                    worker,
                    ..ReaderSpec::filtered(group)
                });
                let baseline = (baselines && group != SAFETY_CURRENT_GROUP)
                    .then(|| ReaderSpec::baseline(group));
                filtered.chain(baseline)
            })
            .filter(|spec| {
                !frostline_consumers::owned_partitions(settings.partitions, workers, spec)
                    .is_empty()
            })
            .collect();
        // Workers of one group are one subscription.
        let subscriptions: Vec<Subscription> = specs
            .iter()
            .map(|spec| Subscription {
                group: spec.group.clone(),
                baseline: spec.baseline,
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let reporter = Reporter::spawn(
            &laser,
            &provisioned.run_file.run_id,
            settings.partitions,
            subscriptions,
            settings.max_pending_windows,
        )
        .await?;
        phase("readers");
        let readers = specs
            .into_iter()
            .map(|spec| {
                let task = tokio::spawn({
                    let (settings, factory, run_file) = (
                        settings.clone(),
                        factory.clone(),
                        provisioned.run_file.clone(),
                    );
                    let (spec, shutdown) = (spec.clone(), shutdown.clone());
                    async move {
                        frostline_consumers::run(
                            &settings, &factory, &run_file, spec, shutdown, None,
                        )
                        .await
                    }
                });
                (spec, task)
            })
            .collect();
        Ok(Self {
            laser,
            provisioned,
            reporter,
            readers,
        })
    }

    pub async fn collect_finished(
        readers: &mut Vec<ReaderTask>,
        lines: &mut Vec<report::ReaderLine>,
    ) -> Result<(), DemoError> {
        for index in (0..readers.len()).rev() {
            if readers[index].1.is_finished() {
                let (spec, task) = readers.swap_remove(index);
                let summary = task.await.map_err(|error| {
                    DemoError::Incomplete(format!("reader {} panicked: {error}", spec.group))
                })??;
                lines.push(report::ReaderLine {
                    group: spec.group,
                    baseline: spec.baseline,
                    worker: spec.worker,
                    summary,
                });
            }
        }
        Ok(())
    }

    /// Wait for every reader, retaining unfinished handles if the wait is cancelled.
    pub async fn join_readers(
        readers: &mut Vec<ReaderTask>,
    ) -> Result<Vec<report::ReaderLine>, DemoError> {
        let mut lines = Vec::new();
        while !readers.is_empty() {
            let result = (&mut readers[0].1).await;
            let (spec, _) = readers.remove(0);
            let summary = result.map_err(|error| {
                DemoError::Incomplete(format!("reader {} panicked: {error}", spec.group))
            })??;
            lines.push(report::ReaderLine {
                group: spec.group,
                baseline: spec.baseline,
                worker: spec.worker,
                summary,
            });
        }
        Ok(lines)
    }

    pub async fn finish(
        settings: &Settings,
        control: ServiceHandle,
        laser: &Laser,
        run: &RunFile,
        readers: &mut Vec<ReaderTask>,
        reporter: Option<Reporter>,
    ) -> Result<(), DemoError> {
        control.cancel();
        let stopped = timeout(settings.drain_timeout, Self::join_readers(readers)).await;
        for (_, task) in readers.iter() {
            task.abort();
        }
        for (_, task) in readers.drain(..) {
            let _ = task.await;
        }
        let reported = match reporter {
            Some(reporter) => reporter.stop().await.map(|_| ()),
            None => Ok(()),
        };
        let removed = if settings.keep_run {
            Ok(())
        } else {
            cleanup(laser, run).await
        };
        let closed = laser.close().await;
        stopped.map_err(|_| {
            DemoError::Incomplete("the readers did not stop before the drain timeout".to_owned())
        })??;
        reported?;
        removed?;
        closed?;
        Ok(())
    }
}

/// Wait for `work` while windows keep completing. `None` once no window completed for `idle`.
pub async fn while_progressing<T>(
    work: impl Future<Output = T>,
    mut completed: watch::Receiver<u64>,
    idle: Duration,
) -> Option<T> {
    tokio::pin!(work);
    loop {
        tokio::select! {
            output = &mut work => return Some(output),
            changed = timeout(idle, completed.changed()) => {
                if !matches!(changed, Ok(Ok(()))) {
                    return None;
                }
            }
        }
    }
}
