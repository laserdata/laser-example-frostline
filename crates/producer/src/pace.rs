use std::time::Duration;
use tokio::time::{Instant, sleep_until};

/// Keeps cumulative deadlines at the requested rate with a bounded catch-up burst.
pub struct RateLimiter {
    interval: Duration,
    next: Instant,
    max_lag: Duration,
}

impl RateLimiter {
    pub fn new(per_second: u32, catch_up_records: u32) -> Self {
        Self {
            interval: Duration::from_secs(1) / per_second.max(1),
            next: Instant::now(),
            max_lag: (Duration::from_secs(1) / per_second.max(1))
                * catch_up_records.saturating_sub(1),
        }
    }

    pub async fn acquire(&mut self) {
        let now = Instant::now();
        self.next = self.next.max(now - self.max_lag);
        if self.next > now {
            sleep_until(self.next).await;
        }
        self.next += self.interval;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn given_a_thousand_per_second_when_acquiring_a_hundred_then_should_take_about_a_tenth_of_a_second()
     {
        let mut limiter = RateLimiter::new(1000, 1);
        let started = Instant::now();
        for _ in 0..100 {
            limiter.acquire().await;
        }
        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_millis(99) && elapsed <= Duration::from_millis(101),
            "{elapsed:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn given_a_late_caller_when_acquiring_then_should_not_burst() {
        let mut limiter = RateLimiter::new(10, 1);
        limiter.acquire().await;
        tokio::time::advance(Duration::from_secs(1)).await;
        let started = Instant::now();
        limiter.acquire().await;
        limiter.acquire().await;
        assert!(started.elapsed() >= Duration::from_millis(100));
    }
}
