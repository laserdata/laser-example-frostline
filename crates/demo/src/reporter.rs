use crate::error::DemoError;
use frostline_shared::domain::WindowId;
use frostline_shared::measure::{Aggregator, Completion, Subscription, Summary};
use frostline_shared::names::RunId;
use frostline_shared::reports::ReportReader;
use laser_sdk::prelude::Laser;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep};

const POLL: Duration = Duration::from_millis(200);

/// Follows the run's `reports` topic and keeps the aggregate current for the board and the report.
pub struct Reporter {
    state: Arc<Mutex<Aggregator>>,
    completed: watch::Receiver<u64>,
    stop: watch::Sender<bool>,
    task: JoinHandle<Result<(), DemoError>>,
}

/// One consistent view of the aggregate.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub summary: Summary,
    pub producer_window: Option<WindowId>,
    pub pending: Option<(WindowId, Completion)>,
    pub expired: u64,
}

impl Reporter {
    pub async fn spawn(
        laser: &Laser,
        run_id: &RunId,
        partitions: u32,
        subscriptions: Vec<Subscription>,
        max_pending: u32,
    ) -> Result<Self, DemoError> {
        let aggregator = Aggregator::new(
            run_id.clone(),
            (0..partitions).collect(),
            subscriptions,
            max_pending as usize,
        );
        let state = Arc::new(Mutex::new(aggregator));
        let (stop, stopped) = watch::channel(false);
        let (windows, completed) = watch::channel(0);
        let reader =
            ReportReader::new(laser, run_id, &format!("frostline-reporter-{run_id}")).await?;
        let task = tokio::spawn(follow(reader, state.clone(), windows, stopped));
        Ok(Self {
            state,
            completed,
            stop,
            task,
        })
    }

    /// How many windows every subscription completed, updated as receipts arrive.
    pub fn completed(&self) -> watch::Receiver<u64> {
        self.completed.clone()
    }

    pub fn snapshot(&self) -> Snapshot {
        snapshot_of(&self.state)
    }

    /// Wait until `windows` windows are complete for every subscription. Fails only when no window
    /// completes for `idle`, so a large backlog that keeps draining never times out.
    pub async fn wait_for(&self, windows: u64, idle: Duration) -> Result<Snapshot, DemoError> {
        let mut deadline = Instant::now() + idle;
        let mut completed = 0;
        loop {
            let snapshot = self.snapshot();
            if snapshot.summary.windows >= windows {
                return Ok(snapshot);
            }
            if snapshot.summary.windows > completed {
                completed = snapshot.summary.windows;
                deadline = Instant::now() + idle;
            }
            if let Some((
                window,
                completion @ (Completion::Conflicting | Completion::Mismatched { .. }),
            )) = &snapshot.pending
            {
                return Err(DemoError::Incomplete(format!(
                    "window {window} is {completion:?}"
                )));
            }
            if Instant::now() >= deadline {
                let waiting = snapshot.pending.map_or_else(
                    || "no report arrived".to_owned(),
                    |(window, completion)| format!("window {window} is {completion:?}"),
                );
                return Err(DemoError::Incomplete(format!(
                    "{} of {windows} windows completed before the drain timeout, {waiting}",
                    snapshot.summary.windows
                )));
            }
            sleep(POLL).await;
        }
    }

    pub async fn stop(self) -> Result<Snapshot, DemoError> {
        let _ = self.stop.send(true);
        self.task.await.map_err(|error| {
            DemoError::Incomplete(format!("the reporter task failed: {error}"))
        })??;
        Ok(snapshot_of(&self.state))
    }
}

fn snapshot_of(state: &Mutex<Aggregator>) -> Snapshot {
    let aggregator = state.lock().expect("the aggregate lock is not poisoned");
    Snapshot {
        summary: aggregator.totals().summary(),
        producer_window: aggregator.producer_window(),
        pending: aggregator.first_pending(),
        expired: aggregator.expired(),
    }
}

async fn follow(
    mut reader: ReportReader,
    state: Arc<Mutex<Aggregator>>,
    windows: watch::Sender<u64>,
    stopped: watch::Receiver<bool>,
) -> Result<(), DemoError> {
    while !*stopped.borrow() {
        if let Some(report) = reader.next(POLL).await? {
            let completed = {
                let mut aggregator = state.lock().expect("the aggregate lock is not poisoned");
                aggregator.apply(report)?;
                aggregator.totals().windows
            };
            windows.send_replace(completed);
        }
    }
    reader.close().await?;
    Ok(())
}
