use frostline_shared::codec::Codec;
use frostline_shared::config::{Catalog, Mode};
use frostline_shared::measure::hex;
use frostline_shared::names::{Role, RunId, RunTopic};
use frostline_shared::runfile::{GroupRecord, RunFile};
use frostline_shared::topology::{self, Retention};
use frostline_shared::{LaserFactory, ServiceHandle, Settings};
use laser_sdk::iggy::prelude::{
    Consumer, ConsumerOffsetClient, GlobalPermissions, Identifier, Permissions, StreamPermissions,
    TopicClient, TopicPermissions, UserClient, UserStatus,
};
use laser_sdk::prelude::Laser;
use std::collections::BTreeMap;

pub const READER_USER: &str = "frostline-reader";
pub const READER_PASSWORD: &str = "frostline-reader-password";

pub fn settings(partitions: u32, total_records: u64, checkpoint_records: u64) -> Settings {
    Settings {
        partitions,
        total_records,
        checkpoint_records,
        rate_per_second: 1_000_000,
        incident_onset_per_mille: 40,
        seed: 5,
        catalog: Catalog::Inline,
        ..Settings::defaults(Mode::Finite)
    }
}

pub async fn provisioned_run(factory: &LaserFactory, settings: &Settings) -> (Laser, RunFile) {
    let run_id = RunId::mint();
    let laser = factory.connect(&run_id.stream()).await.expect("connect");
    let retention = Retention {
        changes: None,
        reports: None,
    };
    let source = topology::ensure_run(&laser, &run_id, settings.partitions, retention)
        .await
        .expect("topology");
    let mut run_file = RunFile {
        run_id,
        settings_digest: settings.digest(),
        generator_version: frostline_producer::fleet::GENERATOR_VERSION.to_owned(),
        host: "127.0.0.1".to_owned(),
        partitions: settings.partitions,
        workers_per_role: 1,
        checkpoint_records: settings.checkpoint_records,
        codec: Codec::Json,
        catalog: Catalog::Inline,
        source,
        schema_ids: Vec::new(),
        groups: Vec::new(),
    };
    for (name, policy) in frostline_shared::policy::by_group() {
        let configured = laser
            .stream(run_file.run_id.stream())
            .topic(RunTopic::Changes.to_string())
            .consumer_group(name)
            .create()
            .filter(policy.filter(run_file.codec, &run_file.schema_ids))
            .build()
            .await
            .expect("group policy is configured");
        let binding = configured.filter.expect("requested group policy");
        run_file.groups.push(GroupRecord {
            group: name.to_owned(),
            digest: hex(&binding.digest.0),
            binding: Some(binding),
        });
    }
    (laser, run_file)
}

pub async fn published_run(factory: &LaserFactory, settings: &Settings) -> (Laser, RunFile) {
    let (laser, run_file) = provisioned_run(factory, settings).await;
    let service = ServiceHandle::new("producer-test");
    frostline_producer::run(settings, factory, &run_file, service.watch(), None)
        .await
        .expect("the producer finishes");
    (laser, run_file)
}

// The reader may read both topics but not write to reports, so every receipt publish is refused.
pub async fn grant_read_only(laser: &Laser, run_file: &RunFile) {
    let stream = Identifier::named(&run_file.run_id.stream()).expect("stream name");
    let reports = laser
        .client()
        .get_topic(
            &stream,
            &Identifier::named(&RunTopic::Reports.to_string()).expect("topic name"),
        )
        .await
        .expect("reports topic reads")
        .expect("reports topic exists");
    let read = TopicPermissions {
        manage_topic: false,
        read_topic: true,
        poll_messages: true,
        send_messages: false,
    };
    let topics = BTreeMap::from([
        (run_file.source.topic_id as usize, read.clone()),
        (reports.id as usize, read),
    ]);
    let stream_permissions = StreamPermissions {
        read_stream: true,
        read_topics: true,
        topics: Some(topics),
        ..StreamPermissions::default()
    };
    let permissions = Permissions {
        global: GlobalPermissions::default(),
        streams: Some(BTreeMap::from([(
            run_file.source.stream_id as usize,
            stream_permissions,
        )])),
    };
    laser
        .client()
        .create_user(
            READER_USER,
            READER_PASSWORD,
            UserStatus::Active,
            Some(permissions),
        )
        .await
        .expect("the read-only user is created");
}

pub async fn stored_offset(laser: &Laser, run_file: &RunFile) -> Option<u64> {
    laser
        .client()
        .get_consumer_offset(
            &Consumer::group(
                Identifier::numeric(
                    run_file
                        .group(Role::Maintenance.group())
                        .expect("maintenance group")
                        .binding
                        .as_ref()
                        .expect("group binding")
                        .identity
                        .group_id
                        .try_into()
                        .expect("native group id"),
                )
                .expect("group id"),
            ),
            &Identifier::named(&run_file.run_id.stream()).expect("stream name"),
            &Identifier::named(&RunTopic::Changes.to_string()).expect("topic name"),
            Some(0),
        )
        .await
        .expect("the offset reads")
        .map(|offset| offset.stored_offset)
}
