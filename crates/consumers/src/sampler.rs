use std::time::{Duration, Instant};

/// Allows at most `per_second` narration lines per second. Zero disables narration.
#[derive(Debug)]
pub struct Sampler {
    per_second: u8,
    window_start: Option<Instant>,
    used: u8,
}

impl Sampler {
    pub fn new(per_second: u8) -> Self {
        Self {
            per_second,
            window_start: None,
            used: 0,
        }
    }

    pub fn allow(&mut self, now: Instant) -> bool {
        if self.per_second == 0 {
            return false;
        }
        let fresh = self
            .window_start
            .is_none_or(|start| now.duration_since(start) >= Duration::from_secs(1));
        if fresh {
            self.window_start = Some(now);
            self.used = 0;
        }
        if self.used < self.per_second {
            self.used += 1;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_two_per_second_when_asked_often_then_should_allow_two_per_second() {
        let start = Instant::now();
        let mut sampler = Sampler::new(2);
        let allowed = (0..5).filter(|_| sampler.allow(start)).count();
        assert_eq!(allowed, 2);
        assert!(sampler.allow(start + Duration::from_secs(1)));
    }

    #[test]
    fn given_zero_when_asked_then_should_never_allow() {
        assert!(!Sampler::new(0).allow(Instant::now()));
    }
}
