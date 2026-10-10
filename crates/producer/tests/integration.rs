use frostline_shared::codec::{Codec, SchemaSet};
use frostline_shared::config::{Catalog, Mode};
use frostline_shared::measure::Report;
use frostline_shared::names::{Frame, Header, RunId, RunTopic};
use frostline_shared::reports::ReportReader;
use frostline_shared::runfile::RunFile;
use frostline_shared::testkit::TestStack;
use frostline_shared::topology::{self, Retention};
use frostline_shared::{ServiceHandle, Settings};
use laser_sdk::prelude::{CommitPolicy, ConsumerStart};
use std::time::Duration;

const RECORDS: u64 = 400;
const WINDOW: u64 = 100;
const PARTITIONS: u32 = 2;
const WAIT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn given_a_finite_run_when_published_then_should_match_what_a_reader_reads_back_manifests() {
    let stack = TestStack::start().await;
    let factory = stack.factory();
    let run_id = RunId::mint();
    let laser = factory.connect(&run_id.stream()).await.expect("connect");
    let retention = Retention {
        changes: None,
        reports: None,
    };
    let source = topology::ensure_run(&laser, &run_id, PARTITIONS, retention)
        .await
        .expect("run topology");
    let settings = Settings {
        partitions: PARTITIONS,
        total_records: RECORDS,
        checkpoint_records: WINDOW,
        rate_per_second: 1_000_000,
        seed: 3,
        ..Settings::defaults(Mode::Finite)
    };
    let run_file = RunFile {
        run_id: run_id.clone(),
        settings_digest: settings.digest(),
        generator_version: frostline_producer::fleet::GENERATOR_VERSION.to_owned(),
        host: "127.0.0.1".to_owned(),
        partitions: PARTITIONS,
        workers_per_role: 1,
        checkpoint_records: settings.checkpoint_records,
        codec: Codec::Json,
        catalog: Catalog::Inline,
        source,
        schema_ids: Vec::new(),
        groups: Vec::new(),
    };
    let service = ServiceHandle::new("producer-test");
    let summary = frostline_producer::run(&settings, &factory, &run_file, service.watch(), None)
        .await
        .expect("the producer finishes");
    assert_eq!(
        (summary.records, summary.windows),
        (RECORDS, RECORDS / WINDOW)
    );

    let mut reports = ReportReader::new(&laser, &run_id, "producer-test-reports")
        .await
        .expect("reports reader");
    let mut manifests = Vec::new();
    while let Some(report) = reports.next(WAIT).await.expect("reports read") {
        if let Report::Manifest(manifest) = report {
            manifests.push(manifest);
        }
    }
    reports.close().await.expect("reports reader closes");
    assert_eq!(manifests.len() as u64, RECORDS / WINDOW);

    for partition in 0..PARTITIONS {
        let mut consumer = laser
            .stream(run_id.stream())
            .topic(RunTopic::Changes.to_string())
            .consumer(format!("reader-{partition}"), partition)
            .start_at(ConsumerStart::First)
            .commit_policy(CommitPolicy::Disabled)
            .build()
            .await
            .expect("ordinary reader");
        let (mut business_bytes, mut checkpoints) = (0u64, 0u64);
        while let Ok(message) = consumer.next_within(WAIT).await {
            let frame = message
                .headers
                .get(&Header::Frame.key())
                .expect("frame header");
            match Frame::from_code(u8::try_from(frame).expect("uint8 frame")) {
                Some(Frame::Checkpoint) => checkpoints += 1,
                Some(Frame::Business) => {
                    let event = Codec::Json
                        .decode(&message.payload, &SchemaSet::default())
                        .expect("event decodes");
                    event.validate().expect("event is valid");
                    business_bytes += message.payload.len() as u64;
                }
                None => panic!("unknown frame"),
            }
        }
        consumer.shutdown().await.expect("reader closes");
        let expected: u64 = manifests
            .iter()
            .flat_map(|manifest| manifest.partitions.iter())
            .filter(|manifest| manifest.partition_id == partition)
            .map(|manifest| manifest.payload_bytes)
            .sum();
        assert_eq!(business_bytes, expected, "partition {partition}");
        assert_eq!(checkpoints, RECORDS / WINDOW, "partition {partition}");
    }
}
