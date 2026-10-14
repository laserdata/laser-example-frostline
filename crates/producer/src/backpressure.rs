use tokio::sync::watch;

// One window is being checkpointed and one manifest can be in flight while the producer waits.
const IN_FLIGHT_WINDOWS: u64 = 2;

/// Holds the producer back while too many published windows still wait for their receipts.
pub struct Backpressure {
    completed: watch::Receiver<u64>,
    max_open: u64,
}

impl Backpressure {
    pub fn new(completed: watch::Receiver<u64>, max_pending_windows: u32) -> Self {
        Self {
            completed,
            max_open: u64::from(max_pending_windows)
                .saturating_sub(IN_FLIGHT_WINDOWS)
                .max(1),
        }
    }

    /// Wait until fewer than the bound of the `published` windows are open. False when the reporter stopped.
    pub async fn admit(&mut self, published: u64) -> bool {
        let max_open = self.max_open;
        self.completed
            .wait_for(|completed| published.saturating_sub(*completed) < max_open)
            .await
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test]
    async fn given_too_many_open_windows_when_admitting_then_should_wait_until_one_completes() {
        let (completed, receiver) = watch::channel(0);
        let mut backpressure = Backpressure::new(receiver, 5);
        assert!(backpressure.admit(2).await);
        assert!(
            timeout(Duration::from_millis(50), backpressure.admit(3))
                .await
                .is_err()
        );
        completed.send_replace(1);
        assert!(backpressure.admit(3).await);
    }

    #[tokio::test]
    async fn given_a_stopped_reporter_when_admitting_then_should_refuse() {
        let (completed, receiver) = watch::channel(0);
        let mut backpressure = Backpressure::new(receiver, 3);
        drop(completed);
        assert!(!backpressure.admit(5).await);
    }
}
