use super::SERVER;
use crate::collect::syscalls::WireUsage;
use crate::trial::{ReadPhase, ReaderWire, Repetition};
use frostline_consumers::latency::LatencySummary;

pub(super) struct ReaderCost {
    pub cpu_seconds: f64,
    pub peak_rss_bytes: f64,
}

// CPU seconds and peak RSS of one group's untraced reader in every repetition.
pub(super) fn readers(
    repetitions: &[Repetition],
    pick: fn(&Repetition) -> &ReadPhase,
    group: &str,
) -> Vec<ReaderCost> {
    repetitions
        .iter()
        .flat_map(|repetition| {
            pick(repetition)
                .readers
                .iter()
                .filter(|reader| reader.group == group)
        })
        .map(|reader| ReaderCost {
            cpu_seconds: reader.usage.cpu_seconds,
            peak_rss_bytes: reader.usage.peak_rss_bytes as f64,
        })
        .collect()
}

pub(super) fn wires(
    repetitions: &[Repetition],
    pick: fn(&Repetition) -> &[ReaderWire],
    group: &str,
) -> Vec<WireUsage> {
    repetitions
        .iter()
        .flat_map(|repetition| {
            pick(repetition)
                .iter()
                .filter(|reader| reader.group == group)
        })
        .map(|reader| reader.wire)
        .collect()
}

pub(super) fn server_cpu(
    repetitions: &[Repetition],
    pick: fn(&Repetition) -> &ReadPhase,
) -> Vec<f64> {
    repetitions
        .iter()
        .flat_map(|repetition| {
            pick(repetition)
                .server
                .iter()
                .filter(|server| server.name == SERVER)
        })
        .map(|server| server.cpu_seconds)
        .collect()
}

// Each percentile is the median of that percentile over the repetitions.
pub(super) fn median_latency(
    repetitions: &[Repetition],
    pick: fn(&Repetition) -> &ReadPhase,
    group: &str,
) -> LatencySummary {
    let summaries: Vec<LatencySummary> = repetitions
        .iter()
        .flat_map(|repetition| {
            pick(repetition)
                .readers
                .iter()
                .filter(|reader| reader.group == group)
        })
        .map(|reader| reader.fetch_latency)
        .collect();
    let of =
        |pick: fn(&LatencySummary) -> f64| median(&summaries.iter().map(pick).collect::<Vec<_>>());
    LatencySummary {
        fetches: of(|summary| summary.fetches as f64) as u64,
        p50_us: of(|summary| f64::from(summary.p50_us)) as u32,
        p95_us: of(|summary| f64::from(summary.p95_us)) as u32,
        p99_us: of(|summary| f64::from(summary.p99_us)) as u32,
        p999_us: of(|summary| f64::from(summary.p999_us)) as u32,
        relative_error_ppm: summaries
            .iter()
            .map(|summary| summary.relative_error_ppm)
            .max()
            .unwrap_or(0),
        max_us: summaries
            .iter()
            .map(|summary| summary.max_us)
            .max()
            .unwrap_or(0),
        examined_per_fetch: of(|summary| summary.examined_per_fetch as f64) as u64,
    }
}

pub(super) fn server_peak(
    repetitions: &[Repetition],
    pick: fn(&Repetition) -> &ReadPhase,
) -> Vec<f64> {
    repetitions
        .iter()
        .flat_map(|repetition| {
            pick(repetition)
                .server
                .iter()
                .filter(|server| server.name == SERVER)
        })
        .map(|server| server.peak_rss_bytes as f64)
        .collect()
}

pub(super) fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    match sorted.len() {
        0 => 0.0,
        length if length % 2 == 1 => sorted[length / 2],
        length => (sorted[length / 2 - 1] + sorted[length / 2]) / 2.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_odd_and_even_samples_when_the_median_is_taken_then_should_pick_the_middle() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), 2.5);
        assert_eq!(median(&[]), 0.0);
    }
}
