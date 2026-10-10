use serde::{Deserialize, Serialize};
use std::time::Duration;

const EXACT_LIMIT: usize = 65_536;
const BUCKETS: usize = 896;
const RELATIVE_ERROR_PPM: u32 = 31_250;

/// Bounded fetch observations. Short trials keep exact samples, long runs use a fixed histogram.
#[derive(Clone, Debug)]
pub struct Latency {
    samples: Option<Vec<u32>>,
    counts: Box<[u64; BUCKETS]>,
    maxima: Box<[u32; BUCKETS]>,
    fetches: u64,
    examined: u64,
    maximum: u32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct LatencySummary {
    pub fetches: u64,
    pub p50_us: u32,
    #[serde(default, skip_serializing)]
    pub p95_us: u32,
    pub p99_us: u32,
    #[serde(default)]
    pub p999_us: u32,
    #[serde(default, skip_serializing)]
    pub max_us: u32,
    pub examined_per_fetch: u64,
    /// Zero for exact samples, otherwise the histogram's maximum relative error.
    #[serde(default)]
    pub relative_error_ppm: u32,
}

impl Latency {
    pub fn record(&mut self, elapsed: Duration, examined: u64) {
        let micros = u32::try_from(elapsed.as_micros()).unwrap_or(u32::MAX);
        let index = bucket(micros);
        self.counts[index] += 1;
        self.maxima[index] = self.maxima[index].max(micros);
        self.fetches += 1;
        self.examined += examined;
        self.maximum = self.maximum.max(micros);
        if let Some(samples) = &mut self.samples {
            if samples.len() < EXACT_LIMIT {
                samples.push(micros);
            } else {
                self.samples = None;
            }
        }
    }

    pub fn summary(&self) -> LatencySummary {
        let sorted = self.samples.as_ref().map(|samples| {
            let mut sorted = samples.clone();
            sorted.sort_unstable();
            sorted
        });
        let rank = |per_mille: u64| -> u32 {
            if self.fetches == 0 {
                return 0;
            }
            let target = (u128::from(self.fetches) * u128::from(per_mille)).div_ceil(1000) as u64;
            if let Some(samples) = &sorted {
                return samples[(target - 1) as usize];
            }
            let mut seen = 0;
            for (index, count) in self.counts.iter().enumerate() {
                seen += count;
                if seen >= target {
                    return self.maxima[index];
                }
            }
            self.maximum
        };
        LatencySummary {
            fetches: self.fetches,
            p50_us: rank(500),
            p95_us: rank(950),
            p99_us: rank(990),
            p999_us: rank(999),
            max_us: self.maximum,
            examined_per_fetch: self.examined / self.fetches.max(1),
            relative_error_ppm: if sorted.is_some() {
                0
            } else {
                RELATIVE_ERROR_PPM
            },
        }
    }
}

impl Default for Latency {
    fn default() -> Self {
        Self {
            samples: Some(Vec::new()),
            counts: Box::new([0; BUCKETS]),
            maxima: Box::new([0; BUCKETS]),
            fetches: 0,
            examined: 0,
            maximum: 0,
        }
    }
}

fn bucket(micros: u32) -> usize {
    if micros < 32 {
        return micros as usize;
    }
    let exponent = 31 - micros.leading_zeros();
    let shift = exponent - 5;
    ((exponent - 4) * 32 + (micros >> shift) - 32) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_one_thousand_fetches_when_summarized_then_should_report_exact_tail_ranks() {
        let mut latency = Latency::default();
        for micros in (1..=1000).rev() {
            latency.record(Duration::from_micros(micros), 250);
        }
        let summary = latency.summary();
        assert_eq!(
            (
                summary.p50_us,
                summary.p95_us,
                summary.p99_us,
                summary.p999_us,
                summary.max_us
            ),
            (500, 950, 990, 999, 1000)
        );
        assert_eq!(summary.examined_per_fetch, 250);
        assert_eq!(summary.relative_error_ppm, 0);
    }

    #[test]
    fn given_long_running_fetches_when_the_sample_limit_is_reached_then_should_keep_bounded_statistics()
     {
        let mut latency = Latency::default();
        for micros in 1..=1_000_000 {
            latency.record(Duration::from_micros(micros), 2);
        }
        let summary = latency.summary();
        assert!(latency.samples.is_none());
        assert_eq!(summary.fetches, 1_000_000);
        assert_eq!(summary.max_us, 1_000_000);
        assert!((500_000..=515_625).contains(&summary.p50_us));
        assert!((999_000..=1_000_000).contains(&summary.p999_us));
        assert_eq!(summary.relative_error_ppm, RELATIVE_ERROR_PPM);
        assert!(bucket(u32::MAX) < BUCKETS);
    }

    #[test]
    fn given_no_fetches_when_summarized_then_should_be_all_zero() {
        assert_eq!(Latency::default().summary(), LatencySummary::default());
    }
}
