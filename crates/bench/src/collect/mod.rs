pub mod metrics;
pub mod perf;
pub mod phase;
pub mod process;
pub mod sockets;
pub mod syscalls;

use process::ProcessSample;
use serde::Serialize;
use sockets::SocketLine;
use std::collections::BTreeMap;

/// What one process used during a trial: kernel socket counters and /proc CPU and memory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct ProcessUsage {
    pub cpu_seconds: f64,
    pub peak_rss_bytes: u64,
    pub received_bytes: u64,
    pub sent_bytes: u64,
    pub sockets: u32,
    /// Sockets that closed before the final snapshot. Their last seen counters are included, so the totals are a lower bound.
    pub socket_gaps: u32,
}

/// The last counters seen for every socket of the tracked processes, across all snapshots of a trial.
#[derive(Debug, Default)]
pub struct SocketLedger {
    snapshots: u64,
    sockets: BTreeMap<(u32, String, String), Seen>,
}

#[derive(Clone, Copy, Debug)]
struct Seen {
    received_bytes: u64,
    acked_bytes: u64,
    snapshot: u64,
}

impl SocketLedger {
    pub fn record(&mut self, lines: &[SocketLine], pids: &[u32]) {
        self.snapshots += 1;
        for line in lines {
            for pid in line.pids.iter().filter(|pid| pids.contains(pid)) {
                self.sockets.insert(
                    (*pid, line.local.clone(), line.peer.clone()),
                    Seen {
                        received_bytes: line.received_bytes,
                        acked_bytes: line.acked_bytes,
                        snapshot: self.snapshots,
                    },
                );
            }
        }
    }

    /// Socket totals of `pid` as of the latest snapshot, with CPU and memory from `sample`.
    pub fn usage(&self, pid: u32, sample: ProcessSample, ticks_per_second: u64) -> ProcessUsage {
        let mut usage = ProcessUsage {
            cpu_seconds: sample.cpu_ticks as f64 / ticks_per_second.max(1) as f64,
            peak_rss_bytes: sample.peak_rss_bytes,
            ..ProcessUsage::default()
        };
        for ((owner, _, _), seen) in &self.sockets {
            if *owner != pid {
                continue;
            }
            usage.sockets += 1;
            usage.received_bytes += seen.received_bytes;
            usage.sent_bytes += seen.acked_bytes;
            if seen.snapshot != self.snapshots {
                usage.socket_gaps += 1;
            }
        }
        usage
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_socket_that_closed_early_when_totalled_then_should_count_it_as_a_gap() {
        let open = |local: &str, received_bytes| SocketLine {
            pids: vec![7],
            local: local.to_owned(),
            peer: "127.0.0.1:8090".to_owned(),
            received_bytes,
            acked_bytes: 10,
        };
        let mut ledger = SocketLedger::default();
        ledger.record(&[open("a", 100), open("b", 50)], &[7]);
        ledger.record(&[open("a", 400)], &[7]);
        let sample = ProcessSample {
            cpu_ticks: 250,
            start_ticks: 1,
            rss_bytes: 1,
            peak_rss_bytes: 2,
        };
        let usage = ledger.usage(7, sample, 100);
        assert_eq!(
            (
                usage.received_bytes,
                usage.sent_bytes,
                usage.sockets,
                usage.socket_gaps
            ),
            (450, 20, 2, 1)
        );
        assert_eq!((usage.cpu_seconds, usage.peak_rss_bytes), (2.5, 2));
        assert_eq!(ledger.usage(8, sample, 100).sockets, 0);
    }
}
