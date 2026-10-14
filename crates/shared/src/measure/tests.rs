use super::*;
use crate::names::RunId;

const RUN: &str = "0badc0de";

#[test]
fn given_every_receipt_when_applied_then_should_fold_the_window_into_the_totals() {
    let mut aggregator = aggregator(8);
    aggregator
        .apply(Report::Manifest(manifest(0)))
        .expect("manifest applies");
    for partition_id in 0..2 {
        aggregator
            .apply(Report::Receipt(receipt(
                0,
                "maintenance",
                partition_id,
                "d",
            )))
            .expect("receipt applies");
    }
    assert_eq!(aggregator.totals().latest, Some(WindowId(0)));
    assert_eq!(aggregator.producer_window(), Some(WindowId(0)));
    let summary = aggregator.totals().summary();
    assert_eq!(summary.source_bytes, 2000);
    assert_eq!(summary.subscriptions[0].received_bytes, 20);
    assert_eq!(summary.filtered_reduction, Reduction::Value(0.99));
    assert_eq!(
        (
            summary.subscriptions[0].saved_bytes,
            summary.filtered_saved_bytes
        ),
        (1980, 1980)
    );
}

#[test]
fn given_byte_counts_when_shown_then_should_pick_the_largest_decimal_unit() {
    let shown: Vec<String> = [
        0_i128,
        999,
        1000,
        14_707_295,
        24_568_069_000,
        3_000_000_000_000,
        -2_500_000,
    ]
    .into_iter()
    .map(|bytes| ByteSize(bytes).to_string())
    .collect();
    assert_eq!(
        shown,
        [
            "0 B", "999 B", "1.00 KB", "14.71 MB", "24.57 GB", "3.00 TB", "-2.50 MB"
        ]
    );
}

#[test]
fn given_a_missing_partition_when_checked_then_should_stay_pending() {
    let mut aggregator = aggregator(8);
    aggregator
        .apply(Report::Manifest(manifest(0)))
        .expect("manifest applies");
    aggregator
        .apply(Report::Receipt(receipt(0, "maintenance", 0, "d")))
        .expect("receipt applies");
    assert_eq!(
        aggregator.completion(WindowId(0)),
        Completion::Pending { missing: 1 }
    );
    assert_eq!(aggregator.totals().latest, None);
}

#[test]
fn given_an_identical_repeat_when_applied_then_should_change_nothing() {
    let mut aggregator = aggregator(8);
    aggregator
        .apply(Report::Manifest(manifest(0)))
        .expect("manifest applies");
    let first = receipt(0, "maintenance", 0, "d");
    let repeat = Receipt {
        attempt: 2,
        ..first.clone()
    };
    aggregator
        .apply(Report::Receipt(first))
        .expect("receipt applies");
    aggregator
        .apply(Report::Receipt(repeat))
        .expect("repeat applies");
    assert_eq!(
        aggregator.completion(WindowId(0)),
        Completion::Pending { missing: 1 }
    );
}

#[test]
fn given_a_conflicting_repeat_when_applied_then_should_mark_the_window() {
    let mut aggregator = aggregator(8);
    aggregator
        .apply(Report::Manifest(manifest(0)))
        .expect("manifest applies");
    aggregator
        .apply(Report::Receipt(receipt(0, "maintenance", 0, "d")))
        .expect("receipt applies");
    let conflicting = Receipt {
        matches: 99,
        ..receipt(0, "maintenance", 0, "d")
    };
    aggregator
        .apply(Report::Receipt(conflicting))
        .expect("conflict applies");
    assert_eq!(aggregator.completion(WindowId(0)), Completion::Conflicting);
}

#[test]
fn given_a_digest_that_differs_from_the_manifest_when_checked_then_should_report_the_mismatch() {
    let mut aggregator = aggregator(8);
    aggregator
        .apply(Report::Manifest(manifest(0)))
        .expect("manifest applies");
    aggregator
        .apply(Report::Receipt(receipt(0, "maintenance", 0, "wrong")))
        .expect("receipt applies");
    aggregator
        .apply(Report::Receipt(receipt(0, "maintenance", 1, "d")))
        .expect("receipt applies");
    assert_eq!(
        aggregator.completion(WindowId(0)),
        Completion::Mismatched {
            group: "maintenance".to_owned(),
            partition_id: 0
        }
    );
}

#[test]
fn given_more_unresolved_windows_than_the_bound_when_applied_then_should_expire_the_oldest() {
    let mut aggregator = aggregator(2);
    for window in 0..3 {
        aggregator
            .apply(Report::Manifest(manifest(window)))
            .expect("manifest applies");
    }
    assert_eq!(aggregator.expired(), 1);
    assert_eq!(
        aggregator.first_pending().map(|(window, _)| window),
        Some(WindowId(1))
    );
}

#[test]
fn given_a_report_of_another_run_when_applied_then_should_refuse_it() {
    let mut aggregator = aggregator(2);
    let mut foreign = manifest(0);
    foreign.run_id = "deadbeef".parse().expect("run id");
    assert!(matches!(
        aggregator.apply(Report::Manifest(foreign)),
        Err(AggregateError::ForeignRun { .. })
    ));
}

#[test]
fn given_reduction_edges_when_computed_then_should_keep_zero_and_negative_honest() {
    assert_eq!(Reduction::compute(0, 0), Reduction::NotApplicable);
    assert_eq!(Reduction::compute(100, 150), Reduction::Value(-0.5));
    assert_eq!(Reduction::compute(100, 0).to_string(), "100.0%");
}

#[test]
fn given_a_receipt_when_serialized_as_a_report_then_should_round_trip() {
    let report = Report::Receipt(receipt(0, "maintenance", 0, "d"));
    let bytes = serde_json::to_vec(&report).expect("report serializes");
    assert_eq!(
        serde_json::from_slice::<Report>(&bytes).expect("report parses"),
        report
    );
}

fn aggregator(max_pending: usize) -> Aggregator {
    Aggregator::new(
        RUN.parse::<RunId>().expect("run id"),
        vec![0, 1],
        vec![Subscription {
            group: "maintenance".to_owned(),
            baseline: false,
        }],
        max_pending,
    )
}

fn manifest(window: u64) -> WindowManifest {
    WindowManifest {
        run_id: RUN.parse().expect("run id"),
        window_id: WindowId(window),
        codec: Codec::Json,
        partitions: (0..2)
            .map(|partition_id| PartitionManifest {
                partition_id,
                records: 10,
                payload_bytes: 1000,
                checkpoint_bytes: 80,
            })
            .collect(),
        expected: (0..2)
            .map(|partition_id| ExpectedMatch {
                group: "maintenance".to_owned(),
                partition_id,
                matches: 1,
                payload_bytes: 10,
                digest: "d".to_owned(),
            })
            .collect(),
    }
}

fn receipt(window: u64, group: &str, partition_id: u32, digest: &str) -> Receipt {
    Receipt {
        run_id: RUN.parse().expect("run id"),
        group: group.to_owned(),
        baseline: false,
        partition_id,
        window_id: WindowId(window),
        checkpoint_offset: 11,
        attempt: 1,
        received_records: 1,
        received_bytes: 10,
        matches: 1,
        matched_bytes: 10,
        digest: digest.to_owned(),
        faults: 0,
        duplicates: 0,
    }
}

#[test]
fn given_a_reader_that_received_the_whole_feed_when_summarized_then_should_report_no_savings() {
    let mut aggregator = aggregator(8);
    aggregator
        .apply(Report::Manifest(manifest(0)))
        .expect("manifest applies");
    for partition_id in 0..2 {
        let everything = Receipt {
            received_bytes: 1000,
            received_records: 10,
            ..receipt(0, "maintenance", partition_id, "d")
        };
        aggregator
            .apply(Report::Receipt(everything))
            .expect("receipt applies");
    }
    let summary = aggregator.totals().summary();
    assert_eq!(summary.subscriptions[0].reduction, Reduction::Value(0.0));
    assert_eq!(summary.filtered_saved_bytes, 0);
}
