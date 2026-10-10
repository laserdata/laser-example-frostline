use frostline_demo::mixed::MixedVerdicts;
use frostline_demo::scenario::finite;
use frostline_shared::Settings;
use frostline_shared::codec::Codec;
use frostline_shared::config::{Catalog, Mode};
use frostline_shared::measure::Reduction;
use frostline_shared::testkit::TestStack;
use std::path::Path;

#[tokio::test]
async fn given_the_managed_story_when_run_then_should_receive_less_than_the_feed_every_team() {
    let stack = TestStack::start().await;
    let directory = tempfile::tempdir().expect("output dir");
    let settings = story(Catalog::Managed, directory.path());
    let report = finite(&settings, &stack.factory(), false)
        .await
        .expect("the finite story completes");
    assert!(report.complete);
    assert_eq!(report.summary.windows, 3);
    assert_eq!(report.summary.subscriptions.len(), 4);
    for subscription in &report.summary.subscriptions {
        assert!(
            matches!(subscription.reduction, Reduction::Value(value) if value > 0.0 && value < 1.0),
            "{}",
            subscription.group
        );
    }
    assert_eq!(report.revision_walkthrough.len(), 3);
    assert!(directory.path().join("report.json").exists());
    let verdicts = |filter: &str,
                    selected: &[u64],
                    rejected: &[u64],
                    unevaluated: &[(u64, &str)]| MixedVerdicts {
        filter: filter.to_owned(),
        selected: selected.to_vec(),
        rejected: rejected.to_vec(),
        unevaluated: unevaluated
            .iter()
            .map(|(offset, reason)| (*offset, (*reason).to_owned()))
            .collect(),
    };
    assert_eq!(
        report.mixed_log,
        vec![
            verdicts("north-pharma", &[0], &[1, 2, 3, 4], &[]),
            verdicts(
                "north-pharma with mismatch pass",
                &[0],
                &[1, 2, 4],
                &[(3, "type_mismatch")]
            ),
            verdicts("depot v1 by glob", &[1, 4], &[0, 2, 3], &[]),
            verdicts("every v1 event ignoring case", &[0, 1, 3, 4], &[2], &[]),
        ]
    );
}

#[tokio::test]
async fn given_the_compare_story_when_run_then_should_agree_filtered_and_full_feed_readers() {
    let stack = TestStack::start().await;
    let directory = tempfile::tempdir().expect("output dir");
    let settings = story(Catalog::Managed, directory.path());
    let report = finite(&settings, &stack.factory(), true)
        .await
        .expect("the compare story completes");
    assert!(report.complete);
    let baselines: Vec<_> = report
        .summary
        .subscriptions
        .iter()
        .filter(|subscription| subscription.baseline)
        .collect();
    assert_eq!(baselines.len(), 3);
    for baseline in baselines {
        assert_eq!(baseline.received_bytes, report.summary.source_bytes);
        let filtered = report
            .summary
            .subscriptions
            .iter()
            .find(|subscription| !subscription.baseline && subscription.group == baseline.group)
            .expect("the filtered twin");
        assert_eq!(filtered.matches, baseline.matches, "{}", baseline.group);
    }
}

#[tokio::test]
async fn given_the_inline_story_when_run_then_should_complete_with_group_owned_filters() {
    let stack = TestStack::start().await;
    let directory = tempfile::tempdir().expect("output dir");
    let settings = story(Catalog::Inline, directory.path());
    let report = finite(&settings, &stack.factory(), false)
        .await
        .expect("the inline story completes");
    assert!(report.complete);
    assert!(report.revision_walkthrough.is_empty());
}

#[tokio::test]
async fn given_the_same_settings_when_run_twice_then_should_repeat_every_count_and_byte() {
    let stack = TestStack::start().await;
    let (first_directory, second_directory) = (
        tempfile::tempdir().expect("output dir"),
        tempfile::tempdir().expect("output dir"),
    );
    let first = finite(
        &story(Catalog::Managed, first_directory.path()),
        &stack.factory(),
        false,
    )
    .await
    .expect("the first run completes");
    let second = finite(
        &story(Catalog::Managed, second_directory.path()),
        &stack.factory(),
        false,
    )
    .await
    .expect("the second run completes");
    assert_ne!(first.run_id, second.run_id);
    assert_eq!(first.settings_digest, second.settings_digest);
    assert_eq!(first.summary, second.summary);
    assert_eq!(
        (first.producer.records, first.producer.payload_bytes),
        (second.producer.records, second.producer.payload_bytes)
    );
}

#[tokio::test]
async fn given_more_windows_than_the_reporter_holds_when_run_then_should_wait_and_nothing_should_expire_the_producer()
 {
    let stack = TestStack::start().await;
    let directory = tempfile::tempdir().expect("output dir");
    let settings = Settings {
        total_records: 6000,
        checkpoint_records: 250,
        max_pending_windows: 4,
        ..story(Catalog::Inline, directory.path())
    };
    let report = finite(&settings, &stack.factory(), false)
        .await
        .expect("the bounded story completes");
    assert!(report.complete);
    assert_eq!((report.summary.windows, report.expired_windows), (24, 0));
}

#[tokio::test]
async fn given_several_workers_per_group_when_run_then_should_equal_one_worker_the_totals() {
    let stack = TestStack::start().await;
    let (single, split) = (
        tempfile::tempdir().expect("output dir"),
        tempfile::tempdir().expect("output dir"),
    );
    let one = Settings {
        partitions: 4,
        ..story(Catalog::Managed, single.path())
    };
    let one = finite(&one, &stack.factory(), false)
        .await
        .expect("one worker completes");
    let three = Settings {
        workers_per_role: 3,
        partitions: 4,
        ..story(Catalog::Managed, split.path())
    };
    let three = finite(&three, &stack.factory(), false)
        .await
        .expect("three workers complete");
    assert!(one.complete && three.complete);
    assert_eq!(three.summary, one.summary);
    assert_eq!(
        three
            .readers
            .iter()
            .filter(|reader| !reader.baseline)
            .count(),
        12
    );
}

#[tokio::test]
async fn given_a_team_that_matches_nothing_when_run_then_should_still_complete_every_window_checkpoints()
 {
    let stack = TestStack::start().await;
    let directory = tempfile::tempdir().expect("output dir");
    let settings = Settings {
        change_percent: 100,
        incident_onset_per_mille: 0,
        ..story(Catalog::Inline, directory.path())
    };
    let report = finite(&settings, &stack.factory(), false)
        .await
        .expect("the story completes");
    assert!(report.complete);
    let maintenance = report
        .summary
        .subscriptions
        .iter()
        .find(|subscription| subscription.group == "maintenance")
        .expect("the maintenance subscription");
    assert_eq!((maintenance.matches, maintenance.received_bytes), (0, 0));
    assert_eq!(maintenance.reduction, Reduction::Value(1.0));
    assert_eq!(report.summary.windows, 3);
}

#[tokio::test]
async fn given_schema_codecs_when_run_then_should_register_and_filter_on_the_run_stream() {
    let stack = TestStack::start().await;
    for codec in [Codec::Avro, Codec::Protobuf] {
        let directory = tempfile::tempdir().expect("output dir");
        let settings = Settings {
            codec,
            ..story(Catalog::Managed, directory.path())
        };
        let report = finite(&settings, &stack.factory(), false)
            .await
            .unwrap_or_else(|error| panic!("the {codec} story completes: {error}"));
        assert!(report.complete, "{codec}");
        assert_eq!(report.summary.subscriptions.len(), 4, "{codec}");
    }
}

fn story(catalog: Catalog, directory: &Path) -> Settings {
    Settings {
        catalog,
        total_records: 3000,
        checkpoint_records: 1000,
        rate_per_second: 1_000_000,
        incident_onset_per_mille: 20,
        partitions: 2,
        sampled_events_per_role_per_second: 0,
        output_directory: Some(directory.to_owned()),
        ..Settings::defaults(Mode::Finite)
    }
}
