use frostline_shared::Settings;
use frostline_shared::codec::Codec;
use frostline_shared::config::{Catalog, LogFormat, Mode};
use std::path::PathBuf;
use std::time::Duration;

const PUBLISH_RATE: u32 = 1_000_000;
const READER_POLL_RECORDS: u32 = 1000;
const WINDOW_MARGIN: u64 = 8;
const DRAIN: Duration = Duration::from_secs(900);

/// One benchmark: a fixed seeded dataset, read once by the filtered groups and once by full-feed readers.
#[derive(Clone, Copy, Debug)]
pub struct Profile {
    pub name: &'static str,
    pub description: &'static str,
    pub records: u64,
    pub partitions: u32,
    pub codec: Codec,
    pub catalog: Catalog,
    pub window_records: u64,
    pub repetitions: u8,
    pub poll_records: u32,
}

pub const ALL: [Profile; 7] = [
    Profile {
        name: "smoke",
        description: "Proves the runner end to end in well under a minute",
        records: 20_000,
        partitions: 2,
        codec: Codec::Json,
        catalog: Catalog::Managed,
        window_records: 5000,
        repetitions: 1,
        poll_records: READER_POLL_RECORDS,
    },
    Profile {
        name: "fleet_1m",
        description: "One million JSON records over four partitions, saved filters",
        records: 1_000_000,
        partitions: 4,
        codec: Codec::Json,
        catalog: Catalog::Managed,
        window_records: 20_000,
        repetitions: 3,
        poll_records: READER_POLL_RECORDS,
    },
    Profile {
        name: "fleet_10m",
        description: "Ten million JSON records over four partitions, saved filters",
        records: 10_000_000,
        partitions: 4,
        codec: Codec::Json,
        catalog: Catalog::Managed,
        window_records: 50_000,
        repetitions: 1,
        poll_records: READER_POLL_RECORDS,
    },
    Profile {
        name: "inline_1m",
        description: "fleet_1m with the inline setup profile and group-owned filters",
        records: 1_000_000,
        partitions: 4,
        codec: Codec::Json,
        catalog: Catalog::Inline,
        window_records: 20_000,
        repetitions: 3,
        poll_records: READER_POLL_RECORDS,
    },
    Profile {
        name: "codec_cbor_1m",
        description: "fleet_1m encoded as CBOR",
        records: 1_000_000,
        partitions: 4,
        codec: Codec::Cbor,
        catalog: Catalog::Managed,
        window_records: 20_000,
        repetitions: 3,
        poll_records: READER_POLL_RECORDS,
    },
    Profile {
        name: "codec_avro_1m",
        description: "fleet_1m encoded as Avro with a registered writer schema",
        records: 1_000_000,
        partitions: 4,
        codec: Codec::Avro,
        catalog: Catalog::Managed,
        window_records: 20_000,
        repetitions: 3,
        poll_records: READER_POLL_RECORDS,
    },
    Profile {
        name: "codec_protobuf_1m",
        description: "fleet_1m encoded as Protobuf with a registered descriptor",
        records: 1_000_000,
        partitions: 4,
        codec: Codec::Protobuf,
        catalog: Catalog::Managed,
        window_records: 20_000,
        repetitions: 3,
        poll_records: READER_POLL_RECORDS,
    },
];

impl Profile {
    pub fn by_name(name: &str) -> Option<&'static Profile> {
        ALL.iter().find(|profile| profile.name == name)
    }

    pub fn windows(&self) -> u64 {
        self.records.div_ceil(self.window_records)
    }

    /// The finite settings of one repetition. Everything that decides the records comes from the profile.
    pub fn settings(&self, output_directory: PathBuf) -> Settings {
        Settings {
            catalog: self.catalog,
            codec: self.codec,
            partitions: self.partitions,
            total_records: self.records,
            checkpoint_records: self.window_records,
            rate_per_second: PUBLISH_RATE,
            poll_records: self.poll_records,
            local_guard: false,
            sampled_events_per_role_per_second: 0,
            max_pending_windows: u32::try_from(self.windows() + WINDOW_MARGIN).unwrap_or(u32::MAX),
            drain_timeout: DRAIN,
            log_format: LogFormat::Json,
            output_directory: Some(output_directory),
            ..Settings::defaults(Mode::Finite)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn given_every_profile_when_listed_then_should_be_unique_and_findable_names() {
        let names: BTreeSet<&str> = ALL.iter().map(|profile| profile.name).collect();
        assert_eq!(names.len(), ALL.len());
        for name in names {
            assert_eq!(
                Profile::by_name(name).map(|profile| profile.name),
                Some(name)
            );
        }
        assert!(Profile::by_name("missing").is_none());
    }

    #[test]
    fn given_a_profile_when_turned_into_settings_then_should_hold_every_window_the_reporter() {
        for profile in &ALL {
            let settings = profile.settings(PathBuf::from("runs/bench"));
            assert!(
                u64::from(settings.max_pending_windows) > profile.windows(),
                "{}",
                profile.name
            );
            assert_eq!(
                (settings.total_records, settings.codec, settings.local_guard),
                (profile.records, profile.codec, false)
            );
        }
    }
}
