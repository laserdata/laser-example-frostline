use frostline_shared::codec::Codec;
use frostline_shared::domain::WindowId;
use frostline_shared::event::FleetEvent;
use frostline_shared::measure::{ExpectedMatch, PartitionManifest, RollingDigest, WindowManifest};
use frostline_shared::names::RunId;
use frostline_shared::policy::RolePolicy;

/// Counts one window of confirmed records and what each group's predicate selects from it.
pub struct WindowTracker {
    window: WindowId,
    size: u64,
    count: u64,
    groups: Vec<(&'static str, RolePolicy)>,
    partitions: Vec<PartitionCounters>,
}

#[derive(Default)]
struct PartitionCounters {
    records: u64,
    payload_bytes: u64,
    checkpoint_bytes: u64,
    expected: Vec<Expected>,
}

#[derive(Default)]
struct Expected {
    matches: u64,
    payload_bytes: u64,
    digest: RollingDigest,
}

impl WindowTracker {
    pub fn new(size: u64, partitions: u32, groups: Vec<(&'static str, RolePolicy)>) -> Self {
        let mut tracker = Self {
            window: WindowId(0),
            size,
            count: 0,
            partitions: Vec::new(),
            groups,
        };
        tracker.reset(partitions);
        tracker
    }

    pub fn window(&self) -> WindowId {
        self.window
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn boundary_reached(&self) -> bool {
        self.count >= self.size
    }

    pub fn observe(&mut self, partition: u32, event: &FleetEvent, payload: &[u8]) {
        self.count += 1;
        let counters = &mut self.partitions[partition as usize];
        counters.records += 1;
        counters.payload_bytes += payload.len() as u64;
        for ((_, policy), expected) in self.groups.iter().zip(counters.expected.iter_mut()) {
            if policy.matches(event) {
                expected.matches += 1;
                expected.payload_bytes += payload.len() as u64;
                expected
                    .digest
                    .push(event.event_id, event.sequence.0, payload);
            }
        }
    }

    pub fn add_checkpoint(&mut self, partition: u32, bytes: u64) {
        self.partitions[partition as usize].checkpoint_bytes += bytes;
    }

    /// Close the window into its manifest and start the next one.
    pub fn close(&mut self, run_id: &RunId, codec: Codec) -> WindowManifest {
        let partitions = std::mem::take(&mut self.partitions);
        let mut manifest = WindowManifest {
            run_id: run_id.clone(),
            window_id: self.window,
            codec,
            partitions: Vec::with_capacity(partitions.len()),
            expected: Vec::new(),
        };
        for (partition_id, counters) in (0u32..).zip(partitions) {
            manifest.partitions.push(PartitionManifest {
                partition_id,
                records: counters.records,
                payload_bytes: counters.payload_bytes,
                checkpoint_bytes: counters.checkpoint_bytes,
            });
            for ((group, _), expected) in self.groups.iter().zip(counters.expected) {
                manifest.expected.push(ExpectedMatch {
                    group: (*group).to_owned(),
                    partition_id,
                    matches: expected.matches,
                    payload_bytes: expected.payload_bytes,
                    digest: expected.digest.finish(),
                });
            }
        }
        let partition_count = manifest.partitions.len() as u32;
        self.window = WindowId(self.window.0 + 1);
        self.count = 0;
        self.reset(partition_count);
        manifest
    }

    fn reset(&mut self, partitions: u32) {
        self.partitions = (0..partitions)
            .map(|_| PartitionCounters {
                expected: self.groups.iter().map(|_| Expected::default()).collect(),
                ..PartitionCounters::default()
            })
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use frostline_shared::fixtures;
    use frostline_shared::names::Role;
    use frostline_shared::policy;

    #[test]
    fn given_a_quiet_partition_when_closed_then_should_still_report_it() {
        let mut tracker = WindowTracker::new(2, 3, policy::by_group());
        tracker.observe(0, &fixtures::enters_unsafe(), b"abc");
        let manifest = tracker.close(&run(), Codec::Json);
        assert_eq!(manifest.partitions.len(), 3);
        assert_eq!(
            manifest.partitions[2],
            PartitionManifest {
                partition_id: 2,
                ..PartitionManifest::default()
            }
        );
    }

    #[test]
    fn given_matching_events_when_observed_then_should_expect_what_the_predicate_selects() {
        let mut tracker = WindowTracker::new(10, 1, policy::by_group());
        tracker.observe(0, &fixtures::enters_unsafe(), b"transition");
        tracker.observe(0, &fixtures::battery_while_unsafe(), b"follow-up");
        let manifest = tracker.close(&run(), Codec::Json);
        let strict = manifest
            .expected(Role::FoodSafety.group(), 0)
            .expect("food safety expected");
        let current = manifest
            .expected("food-safety-current", 0)
            .expect("current expected");
        assert_eq!((strict.matches, strict.payload_bytes), (1, 10));
        assert_eq!(current.matches, 2);
    }

    #[test]
    fn given_a_closed_window_when_observing_again_then_should_start_the_next_window_at_zero() {
        let mut tracker = WindowTracker::new(1, 1, policy::by_group());
        tracker.observe(0, &fixtures::reefer_fault(), b"x");
        assert!(tracker.boundary_reached());
        tracker.close(&run(), Codec::Json);
        assert_eq!(tracker.window(), WindowId(1));
        assert!(tracker.is_empty());
        let manifest = tracker.close(&run(), Codec::Json);
        assert_eq!(manifest.records(), 0);
    }

    fn run() -> RunId {
        fixtures::RUN.parse().expect("run id")
    }
}
