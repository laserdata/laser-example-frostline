use super::*;
use crate::codec::Codec;
use crate::measure::{ExpectedMatch, PartitionManifest};

#[test]
fn given_a_later_complete_window_when_the_first_is_missing_then_should_wait_for_the_contiguous_prefix()
 {
    let mut aggregate = aggregate();
    complete(&mut aggregate, 1);
    assert_eq!(aggregate.totals().latest, None);
    assert_eq!(
        aggregate.first_pending(),
        Some((WindowId(0), Completion::Pending { missing: 2 }))
    );
    complete(&mut aggregate, 0);
    assert_eq!(aggregate.totals().latest, Some(WindowId(1)));
    assert_eq!(aggregate.totals().summary().windows, 2);
    assert_eq!(aggregate.first_pending(), None);
}

#[test]
fn given_folded_windows_when_late_reports_repeat_then_should_keep_totals_and_pending_memory_unchanged()
 {
    let mut aggregate = aggregate();
    complete(&mut aggregate, 0);
    let before = aggregate.totals().summary();
    for _ in 0..1000 {
        complete(&mut aggregate, 0);
    }
    assert_eq!(aggregate.totals().summary(), before);
    assert_eq!(aggregate.first_pending(), None);
    assert_eq!(aggregate.expired(), 0);
}

#[test]
fn given_a_correct_digest_when_match_counters_disagree_then_should_reject_the_receipt() {
    for (matches, matched_bytes) in [(2, 10), (1, 20)] {
        let mut aggregate = aggregate();
        aggregate
            .apply(Report::Manifest(manifest(0)))
            .expect("manifest applies");
        aggregate
            .apply(Report::Receipt(Receipt {
                matches,
                matched_bytes,
                ..receipt(0)
            }))
            .expect("receipt applies");
        assert_eq!(
            aggregate.completion(WindowId(0)),
            Completion::Mismatched {
                group: "maintenance".to_owned(),
                partition_id: 0
            }
        );
        assert_eq!(aggregate.totals().latest, None);
    }
}

#[test]
fn given_an_expired_gap_when_later_windows_finish_then_should_resume_without_counting_the_gap() {
    let mut aggregate = aggregate();
    aggregate.max_pending = 1;
    aggregate
        .apply(Report::Manifest(manifest(0)))
        .expect("first window opens");
    aggregate
        .apply(Report::Manifest(manifest(1)))
        .expect("second window evicts the first");
    assert_eq!(aggregate.expired(), 1);
    assert_eq!(aggregate.totals().latest, None);
    aggregate
        .apply(Report::Receipt(receipt(1)))
        .expect("second window completes");
    assert_eq!(aggregate.totals().latest, Some(WindowId(1)));
    assert_eq!(aggregate.totals().summary().windows, 1);
    complete(&mut aggregate, 0);
    complete(&mut aggregate, 2);
    assert_eq!(aggregate.totals().summary().windows, 2);
    assert_eq!(aggregate.expired(), 1);
    assert_eq!(aggregate.first_pending(), None);
}

fn aggregate() -> Aggregator {
    Aggregator::new(
        "0badc0de".parse().expect("run id"),
        vec![0],
        vec![Subscription {
            group: "maintenance".to_owned(),
            baseline: false,
        }],
        8,
    )
}

fn complete(aggregate: &mut Aggregator, window: u64) {
    aggregate
        .apply(Report::Manifest(manifest(window)))
        .expect("manifest applies");
    aggregate
        .apply(Report::Receipt(receipt(window)))
        .expect("receipt applies");
}

fn manifest(window: u64) -> WindowManifest {
    WindowManifest {
        run_id: "0badc0de".parse().expect("run id"),
        window_id: WindowId(window),
        codec: Codec::Json,
        partitions: vec![PartitionManifest {
            partition_id: 0,
            records: 10,
            payload_bytes: 100,
            checkpoint_bytes: 10,
        }],
        expected: vec![ExpectedMatch {
            group: "maintenance".to_owned(),
            partition_id: 0,
            matches: 1,
            payload_bytes: 10,
            digest: "d".to_owned(),
        }],
    }
}

fn receipt(window: u64) -> Receipt {
    Receipt {
        run_id: "0badc0de".parse().expect("run id"),
        window_id: WindowId(window),
        group: "maintenance".to_owned(),
        baseline: false,
        partition_id: 0,
        checkpoint_offset: 11,
        attempt: 1,
        received_records: 1,
        received_bytes: 10,
        matches: 1,
        matched_bytes: 10,
        digest: "d".to_owned(),
        faults: 0,
        duplicates: 0,
    }
}
