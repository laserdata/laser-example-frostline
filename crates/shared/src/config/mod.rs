use crate::codec::Codec;
use crate::knobs;
use crate::measure::hex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::Duration;
use strum::{Display, EnumString};

mod env;

pub use env::incident_updates;

const DEFAULT_SEED: u64 = 1312;
const LIVE_RATE: u32 = 250;
const FINITE_RATE: u32 = 1000;
const HOUR: u64 = 3600;
const DAY: u64 = 86_400;

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Display, EnumString, Eq, PartialEq, Serialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Mode {
    Finite,
    #[default]
    Live,
    Compare,
    Codecs,
}

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Display, EnumString, Eq, PartialEq, Serialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Catalog {
    #[default]
    Managed,
    Inline,
}

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Display, EnumString, Eq, PartialEq, Serialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum LogFormat {
    #[default]
    Pretty,
    Json,
}

/// Every Frostline setting, read once from `FROSTLINE_*` variables and validated.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Settings {
    pub mode: Mode,
    pub catalog: Catalog,
    pub codec: Codec,
    pub seed: u64,
    pub fleet_size: u32,
    pub partitions: u32,
    pub rate_per_second: u32,
    pub rate_catch_up_records: u32,
    pub total_records: u64,
    pub duration: Option<Duration>,
    pub checkpoint_records: u64,
    pub change_percent: u8,
    pub incident_onset_per_mille: u16,
    pub incident_min_updates: u16,
    pub incident_max_updates: u16,
    pub diagnostics_samples: u16,
    pub batch_records: u32,
    pub batch_bytes: u32,
    pub batch_linger: Duration,
    pub publish_queue_records: u32,
    pub poll_records: u32,
    pub reply_bytes: u32,
    pub idle_interval: Duration,
    pub workers_per_role: u8,
    pub local_guard: bool,
    pub max_pending_windows: u32,
    pub board_interval: Duration,
    pub sampled_events_per_role_per_second: u8,
    pub changes_expiry: Option<Duration>,
    pub reports_expiry: Option<Duration>,
    pub drain_timeout: Duration,
    pub readiness_timeout: Duration,
    pub log_format: LogFormat,
    pub output_directory: Option<PathBuf>,
    pub keep_run: bool,
}

impl Settings {
    /// The documented defaults for `mode`, before any variable applies.
    pub fn defaults(mode: Mode) -> Self {
        let finite = matches!(mode, Mode::Finite | Mode::Compare | Mode::Codecs);
        Self {
            mode,
            catalog: Catalog::Managed,
            codec: Codec::Json,
            seed: DEFAULT_SEED,
            fleet_size: 800,
            partitions: 4,
            rate_catch_up_records: 64,
            rate_per_second: if finite { FINITE_RATE } else { LIVE_RATE },
            total_records: 24_000,
            duration: None,
            checkpoint_records: 2000,
            change_percent: 80,
            incident_onset_per_mille: 3,
            incident_min_updates: 5,
            incident_max_updates: 30,
            diagnostics_samples: 32,
            batch_records: 100,
            batch_bytes: 262_144,
            batch_linger: Duration::from_millis(5),
            publish_queue_records: 2000,
            poll_records: 100,
            reply_bytes: 1_048_576,
            idle_interval: Duration::from_millis(100),
            workers_per_role: 1,
            local_guard: finite,
            max_pending_windows: 120,
            board_interval: Duration::from_secs(4),
            sampled_events_per_role_per_second: 2,
            changes_expiry: (!finite).then(|| Duration::from_secs(HOUR)),
            reports_expiry: Some(Duration::from_secs(DAY)),
            drain_timeout: Duration::from_secs(30),
            readiness_timeout: Duration::from_secs(60),
            log_format: LogFormat::Pretty,
            output_directory: None,
            keep_run: false,
        }
    }

    /// A stable fingerprint of every setting except where the report is written, recorded in each report.
    /// Identity of the published dataset: the variables that decide which
    /// records a run holds. Run control such as the mode, the duration, the
    /// rate, logging, and timeouts stays out, so a live producer or a reader
    /// with other local settings can join a finite run.
    pub fn digest(&self) -> String {
        let canonical = serde_json::to_vec(&self.dataset_variables()).expect("settings serialize");
        hex(&Sha256::digest(canonical))
    }

    /// Every variable that decides which records a run publishes, with this run's values.
    pub fn dataset_variables(&self) -> Vec<(&'static str, String)> {
        vec![
            (knobs::CODEC, self.codec.to_string()),
            (knobs::CATALOG, self.catalog.to_string()),
            (knobs::SEED, self.seed.to_string()),
            (knobs::FLEET_SIZE, self.fleet_size.to_string()),
            (knobs::PARTITIONS, self.partitions.to_string()),
            (knobs::TOTAL_RECORDS, self.total_records.to_string()),
            (
                knobs::CHECKPOINT_RECORDS,
                self.checkpoint_records.to_string(),
            ),
            (knobs::CHANGE_PERCENT, self.change_percent.to_string()),
            (
                knobs::INCIDENT_ONSET_PER_MILLE,
                self.incident_onset_per_mille.to_string(),
            ),
            (
                knobs::INCIDENT_UPDATES,
                format!(
                    "{}..{}",
                    self.incident_min_updates, self.incident_max_updates
                ),
            ),
            (
                knobs::DIAGNOSTICS_SAMPLES,
                self.diagnostics_samples.to_string(),
            ),
        ]
    }

    /// Label and value pairs for the startup banner. Settings hold no secrets.
    pub fn summary(&self) -> Vec<(&'static str, String)> {
        vec![
            ("mode", self.mode.to_string()),
            ("catalog", self.catalog.to_string()),
            ("codec", self.codec.to_string()),
            (
                "fleet",
                format!("{} trucks, seed {}", self.fleet_size, self.seed),
            ),
            ("partitions", self.partitions.to_string()),
            (
                "rate",
                format!("{} records per second requested", self.rate_per_second),
            ),
            ("window", format!("{} records", self.checkpoint_records)),
            (
                "local guard",
                if self.local_guard { "on" } else { "off" }.to_owned(),
            ),
        ]
    }
}
