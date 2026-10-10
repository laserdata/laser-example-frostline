use laser_sdk::prelude::LaserError;
use std::future::Future;
use std::time::Duration;
#[cfg(unix)]
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::watch;
use tokio::time::{Instant, sleep};
use tracing::{info, warn};

pub struct ServiceHandle {
    name: &'static str,
    shutdown: watch::Sender<bool>,
}

impl ServiceHandle {
    pub fn new(name: &'static str) -> Self {
        let (shutdown, _) = watch::channel(false);
        Self { name, shutdown }
    }

    pub fn watch(&self) -> ShutdownWatch {
        ShutdownWatch {
            signal: self.shutdown.subscribe(),
        }
    }

    pub fn cancel(&self) {
        self.shutdown.send_replace(true);
        info!(
            service = self.name,
            "Service '{}' shutdown requested", self.name
        );
    }
}

#[derive(Clone)]
pub struct ShutdownWatch {
    signal: watch::Receiver<bool>,
}

impl ShutdownWatch {
    pub async fn cancelled(&mut self) {
        while !*self.signal.borrow() {
            if self.signal.changed().await.is_err() {
                break;
            }
        }
    }
}

pub async fn shutdown_signal() {
    let ctrl_c = async {
        if tokio::signal::ctrl_c().await.is_err() {
            warn!("failed to listen for ctrl-c");
        }
    };
    #[cfg(unix)]
    {
        let mut term = signal(SignalKind::terminate()).expect("install the SIGTERM handler");
        tokio::select! {
            _ = ctrl_c => {},
            _ = term.recv() => {},
        }
    }
    #[cfg(not(unix))]
    {
        ctrl_c.await;
    }
}

pub async fn eventually<F, Fut>(timeout: Duration, mut probe: F) -> Result<(), LaserError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = Instant::now() + timeout;
    loop {
        if probe().await {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(LaserError::Invalid(
                "condition was not met before the deadline".to_owned(),
            ));
        }
        sleep(Duration::from_millis(20)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn given_cancelled_service_when_a_new_watch_subscribes_then_should_observe_cancellation()
    {
        let service = ServiceHandle::new("test");
        service.cancel();
        let mut watch = service.watch();
        tokio::time::timeout(Duration::from_millis(50), watch.cancelled())
            .await
            .expect("cancellation is retained");
    }

    #[tokio::test]
    async fn given_a_condition_that_never_holds_when_awaited_then_should_time_out() {
        assert!(
            eventually(Duration::ZERO, || async { false })
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn given_a_condition_that_holds_when_awaited_then_should_complete() {
        eventually(Duration::from_secs(1), || async { true })
            .await
            .expect("condition holds");
    }
}
