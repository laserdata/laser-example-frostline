mod support;

use frostline_consumers::{ConsumerError, ReaderSpec};
use frostline_shared::codec::{Codec, SchemaSet};
use frostline_shared::fixtures;
use frostline_shared::measure::{Aggregator, Completion, Subscription};
use frostline_shared::names::{Role, RunTopic};
use frostline_shared::reports::ReportReader;
use frostline_shared::testkit::TestStack;
use frostline_shared::{ServiceHandle, Settings};
use laser_sdk::prelude::{ProducerMessage, Routing};
use std::time::Duration;
use support::{grant_read_only, provisioned_run, published_run, settings, stored_offset};

const PARTITIONS: u32 = 2;
const WAIT: Duration = Duration::from_secs(3);

#[tokio::test]
async fn given_a_finite_run_when_every_team_reads_then_should_match_the_manifests_receipts() {
    let stack = TestStack::start().await;
    let factory = stack.factory();
    let settings = settings(PARTITIONS, 600, 200);
    let (laser, run_file) = published_run(&factory, &settings).await;
    let service = ServiceHandle::new("consumers-test");
    let mut specs: Vec<ReaderSpec> = [Role::FoodSafety, Role::Maintenance, Role::Regional]
        .iter()
        .map(|role| ReaderSpec::filtered(role.group()))
        .collect();
    specs.push(ReaderSpec::baseline(Role::FoodSafety.group()));
    let mut subscriptions = Vec::new();
    for spec in specs {
        subscriptions.push(Subscription {
            group: spec.group.clone(),
            baseline: spec.baseline,
        });
        let summary = frostline_consumers::run(
            &settings,
            &factory,
            &run_file,
            spec.clone(),
            service.watch(),
            None,
        )
        .await
        .expect("the reader finishes");
        assert_eq!(
            summary.receipts,
            u64::from(PARTITIONS) * 3,
            "{}",
            spec.group
        );
        assert_eq!(summary.duplicates, 0, "{}", spec.group);
    }

    let run_id = &run_file.run_id;
    let mut aggregator =
        Aggregator::new(run_id.clone(), (0..PARTITIONS).collect(), subscriptions, 16);
    let mut reports = ReportReader::new(&laser, run_id, "consumers-test-reports")
        .await
        .expect("reports reader");
    while let Some(report) = reports.next(WAIT).await.expect("reports read") {
        aggregator.apply(report).expect("report applies");
    }
    reports.close().await.expect("reports reader closes");
    assert_eq!(
        aggregator.first_pending().map(|(_, completion)| completion),
        None::<Completion>
    );
    let summary = aggregator.totals().summary();
    assert_eq!(summary.windows, 3);
    for subscription in &summary.subscriptions {
        if subscription.baseline {
            assert_eq!(subscription.received_bytes, summary.source_bytes);
        } else {
            assert!(
                subscription.received_bytes < summary.source_bytes,
                "{}",
                subscription.group
            );
        }
    }
}

#[tokio::test]
async fn given_a_payload_that_does_not_decode_when_read_then_should_stop_before_any_acknowledgment()
{
    let stack = TestStack::start().await;
    let factory = stack.factory();
    let settings = Settings {
        poll_records: 1,
        ..settings(1, 2, 2)
    };
    let (laser, run_file) = provisioned_run(&factory, &settings).await;
    let fault = fixtures::reefer_fault();
    let headers = Codec::Json
        .headers(&fault, &SchemaSet::default())
        .expect("headers");
    laser
        .stream(run_file.run_id.stream())
        .topic(RunTopic::Changes.to_string())
        .producer()
        .create_stream(false)
        .create_topic(false)
        .build()
        .await
        .expect("producer")
        .send_batch_with_routing(
            [
                ProducerMessage::new(
                    Codec::Json
                        .encode(&fault, &SchemaSet::default())
                        .expect("valid fleet event"),
                )
                .with_headers(headers.clone()),
                ProducerMessage::new(b"not a fleet event".as_slice()).with_headers(headers),
            ],
            Some(Routing::Partition(0)),
        )
        .await
        .expect("the malformed record publishes");

    let service = ServiceHandle::new("decode-test");
    let read = frostline_consumers::run(
        &settings,
        &factory,
        &run_file,
        ReaderSpec::filtered(Role::Maintenance.group()),
        service.watch(),
        None,
    )
    .await;
    assert!(matches!(read, Err(ConsumerError::Codec(_))), "{read:?}");
    assert_eq!(stored_offset(&laser, &run_file).await, None);
}

#[tokio::test]
async fn given_a_receipt_that_cannot_be_published_when_a_checkpoint_arrives_then_should_not_acknowledge_it()
 {
    let stack = TestStack::start().await;
    let factory = stack.factory();
    let settings = settings(1, 200, 100);
    let (laser, run_file) = published_run(&factory, &settings).await;
    grant_read_only(&laser, &run_file).await;

    let reader = stack.factory_as(support::READER_USER, support::READER_PASSWORD);
    let service = ServiceHandle::new("receipt-test");
    let read = frostline_consumers::run(
        &settings,
        &reader,
        &run_file,
        ReaderSpec::filtered(Role::Maintenance.group()),
        service.watch(),
        None,
    )
    .await;
    assert!(matches!(read, Err(ConsumerError::Laser(_))), "{read:?}");
    let first_checkpoint = settings.checkpoint_records;
    let stored = stored_offset(&laser, &run_file).await;
    assert!(
        stored.is_none_or(|offset| offset < first_checkpoint),
        "stored {stored:?}"
    );
}
