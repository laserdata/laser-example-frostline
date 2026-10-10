use frostline_shared::codec::{Codec, SchemaSet};
use frostline_shared::fixtures;
use frostline_shared::names::{Role, RunId, RunTopic};
use frostline_shared::policy::RolePolicy;
use frostline_shared::testkit::TestStack;
use frostline_shared::topology::{self, Retention};
use laser_sdk::filters::FilteredStart;
use laser_sdk::prelude::{ProducerMessage, Routing};
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(15);

#[tokio::test]
async fn given_mixed_records_when_read_with_the_maintenance_filter_then_should_deliver_only_faults_and_checkpoints()
 {
    let stack = TestStack::start().await;
    let run_id = RunId::mint();
    let laser = stack
        .factory()
        .connect(&run_id.stream())
        .await
        .expect("connect");
    let retention = Retention {
        changes: None,
        reports: None,
    };
    topology::ensure_run(&laser, &run_id, 1, retention)
        .await
        .expect("run topology");
    topology::verify_retention(&laser, &run_id, retention)
        .await
        .expect("retention applied");
    let events = [
        fixtures::enters_unsafe(),
        fixtures::battery_while_unsafe(),
        fixtures::reefer_fault(),
        fixtures::retired(frostline_shared::domain::Cargo::Pharma),
        fixtures::checkpoint(0),
    ];
    let schemas = SchemaSet::default();
    let producer = laser
        .stream(run_id.stream())
        .topic(RunTopic::Changes.to_string())
        .producer()
        .create_topic(false)
        .build()
        .await
        .expect("producer");
    let messages = events.iter().map(|event| {
        ProducerMessage::new(Codec::Json.encode(event, &schemas).expect("encodes"))
            .with_headers(Codec::Json.headers(event, &schemas).expect("headers"))
    });
    producer
        .send_batch_with_routing(messages, Some(Routing::Partition(0)))
        .await
        .expect("publish");

    let filter = RolePolicy::for_role(Role::Maintenance).filter(Codec::Json, &[]);
    let group = laser
        .stream(run_id.stream())
        .topic(RunTopic::Changes.to_string())
        .consumer_group("filtered");
    group
        .create()
        .filter(filter)
        .build()
        .await
        .expect("configured group");
    let mut reader = group
        .reader()
        .expect("group reader")
        .start(FilteredStart::First)
        .local_guard(true)
        .build()
        .await
        .expect("filtered reader");
    let mut offsets = Vec::new();
    for _ in 0..3 {
        let record = tokio::time::timeout(WAIT, reader.next_record())
            .await
            .expect("a record arrives")
            .expect("the read succeeds");
        offsets.push(record.offset);
        reader.ack(&record).await.expect("ack");
    }
    reader.close().await.expect("reader closes");
    assert_eq!(offsets, [0, 2, 4]);
    assert!(
        topology::delete_run(&laser, &run_id)
            .await
            .expect("delete run")
    );
}
