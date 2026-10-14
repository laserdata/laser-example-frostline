use super::Settings;
use crate::config::Mode;
use crate::knobs::{self, ConfigError};
use std::path::PathBuf;
use std::time::Duration;

impl Settings {
    pub fn from_env(mode: Mode) -> Result<Self, ConfigError> {
        let base = Self::defaults(mode);
        let (incident_min_updates, incident_max_updates) = incident_updates(
            knobs::read(knobs::INCIDENT_UPDATES),
            (base.incident_min_updates, base.incident_max_updates),
        )?;
        Ok(Self {
            mode,
            catalog: knobs::parsed(knobs::CATALOG, base.catalog)?,
            codec: knobs::parsed(knobs::CODEC, base.codec)?,
            seed: knobs::ranged(knobs::SEED, base.seed, 0, u64::MAX)?,
            fleet_size: knobs::ranged(knobs::FLEET_SIZE, base.fleet_size, 1, 100_000)?,
            partitions: knobs::ranged(knobs::PARTITIONS, base.partitions, 1, 64)?,
            rate_per_second: knobs::ranged(
                knobs::RATE_PER_SECOND,
                base.rate_per_second,
                1,
                1_000_000,
            )?,
            rate_catch_up_records: knobs::ranged(
                knobs::RATE_CATCH_UP_RECORDS,
                base.rate_catch_up_records,
                1,
                1000,
            )?,
            total_records: knobs::ranged(knobs::TOTAL_RECORDS, base.total_records, 1, u64::MAX)?,
            duration: knobs::optional_seconds(
                knobs::DURATION_SECONDS,
                base.duration.map_or(0, |duration| duration.as_secs()),
            )?,
            checkpoint_records: knobs::ranged(
                knobs::CHECKPOINT_RECORDS,
                base.checkpoint_records,
                1,
                u64::MAX,
            )?,
            change_percent: knobs::ranged(knobs::CHANGE_PERCENT, base.change_percent, 0, 100)?,
            incident_onset_per_mille: knobs::ranged(
                knobs::INCIDENT_ONSET_PER_MILLE,
                base.incident_onset_per_mille,
                0,
                1000,
            )?,
            incident_min_updates,
            incident_max_updates,
            diagnostics_samples: knobs::ranged(
                knobs::DIAGNOSTICS_SAMPLES,
                base.diagnostics_samples,
                0,
                1024,
            )?,
            batch_records: knobs::ranged(knobs::BATCH_RECORDS, base.batch_records, 1, 1000)?,
            batch_bytes: knobs::ranged(knobs::BATCH_BYTES, base.batch_bytes, 1024, 8_388_608)?,
            batch_linger: Duration::from_millis(knobs::ranged(
                knobs::BATCH_LINGER_MS,
                base.batch_linger.as_millis() as u64,
                0,
                1000,
            )?),
            publish_queue_records: knobs::ranged(
                knobs::PUBLISH_QUEUE_RECORDS,
                base.publish_queue_records,
                1,
                u32::MAX,
            )?,
            poll_records: knobs::ranged(knobs::POLL_RECORDS, base.poll_records, 1, 1000)?,
            reply_bytes: knobs::ranged(knobs::REPLY_BYTES, base.reply_bytes, 1024, 8_388_608)?,
            idle_interval: Duration::from_millis(knobs::ranged(
                knobs::IDLE_INTERVAL_MS,
                base.idle_interval.as_millis() as u64,
                1,
                u64::MAX,
            )?),
            workers_per_role: knobs::ranged(knobs::WORKERS_PER_ROLE, base.workers_per_role, 1, 8)?,
            local_guard: knobs::flag(knobs::LOCAL_GUARD, base.local_guard)?,
            max_pending_windows: knobs::ranged(
                knobs::MAX_PENDING_WINDOWS,
                base.max_pending_windows,
                1,
                u32::MAX,
            )?,
            board_interval: Duration::from_secs(knobs::ranged(
                knobs::BOARD_INTERVAL_SECONDS,
                base.board_interval.as_secs(),
                1,
                u64::MAX,
            )?),
            sampled_events_per_role_per_second: knobs::ranged(
                knobs::SAMPLED_EVENTS,
                base.sampled_events_per_role_per_second,
                0,
                100,
            )?,
            changes_expiry: knobs::optional_seconds(
                knobs::CHANGES_EXPIRY_SECONDS,
                base.changes_expiry.map_or(0, |expiry| expiry.as_secs()),
            )?,
            reports_expiry: knobs::optional_seconds(
                knobs::REPORTS_EXPIRY_SECONDS,
                base.reports_expiry.map_or(0, |expiry| expiry.as_secs()),
            )?,
            drain_timeout: Duration::from_secs(knobs::ranged(
                knobs::DRAIN_TIMEOUT_SECONDS,
                base.drain_timeout.as_secs(),
                1,
                u64::MAX,
            )?),
            readiness_timeout: Duration::from_secs(knobs::ranged(
                knobs::READINESS_TIMEOUT_SECONDS,
                base.readiness_timeout.as_secs(),
                1,
                u64::MAX,
            )?),
            log_format: knobs::parsed(knobs::LOG_FORMAT, base.log_format)?,
            output_directory: knobs::read(knobs::OUTPUT_DIRECTORY).map(PathBuf::from),
            keep_run: knobs::flag(knobs::KEEP_RUN, base.keep_run)?,
        })
    }
}

/// `min..max` update counts an incident lasts.
pub fn incident_updates(
    raw: Option<String>,
    default: (u16, u16),
) -> Result<(u16, u16), ConfigError> {
    let Some(raw) = raw else {
        return Ok(default);
    };
    let invalid = || {
        ConfigError::invalid(
            knobs::INCIDENT_UPDATES,
            raw.clone(),
            "expected min..max with 1 <= min <= max",
        )
    };
    let (min, max) = raw.split_once("..").ok_or_else(invalid)?;
    let min: u16 = min.trim().parse().map_err(|_| invalid())?;
    let max: u16 = max.trim().parse().map_err(|_| invalid())?;
    if min == 0 || min > max {
        return Err(invalid());
    }
    Ok((min, max))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_finite_and_live_modes_when_defaulted_then_should_differ_only_where_documented() {
        let finite = Settings::defaults(Mode::Finite);
        let live = Settings::defaults(Mode::Live);
        assert_eq!((finite.rate_per_second, live.rate_per_second), (1000, 250));
        assert_eq!((finite.local_guard, live.local_guard), (true, false));
        assert_eq!(finite.changes_expiry, None);
        assert_eq!(live.changes_expiry, Some(Duration::from_secs(3600)));
        assert_eq!(finite.total_records, 24_000);
    }

    #[test]
    fn given_incident_ranges_when_parsed_then_should_accept_only_ordered_positive_bounds() {
        assert_eq!(incident_updates(None, (5, 30)), Ok((5, 30)));
        assert_eq!(
            incident_updates(Some("2..9".to_owned()), (5, 30)),
            Ok((2, 9))
        );
        for bad in ["9..2", "0..3", "5", "a..b"] {
            assert!(
                incident_updates(Some(bad.to_owned()), (5, 30)).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn given_equal_settings_when_digested_then_should_match_and_a_change_should_differ() {
        let settings = Settings::defaults(Mode::Finite);
        assert_eq!(settings.digest(), Settings::defaults(Mode::Finite).digest());
        let changed = Settings {
            seed: 7,
            ..settings.clone()
        };
        assert_ne!(settings.digest(), changed.digest());
        assert_eq!(settings.digest().len(), 64);
        let elsewhere = Settings {
            output_directory: Some("runs/elsewhere".into()),
            ..settings.clone()
        };
        assert_eq!(settings.digest(), elsewhere.digest());
        let live = Settings {
            mode: Mode::Live,
            duration: None,
            keep_run: true,
            ..settings.clone()
        };
        assert_eq!(
            settings.digest(),
            live.digest(),
            "run control is not part of the dataset identity"
        );
    }

    #[test]
    fn given_default_settings_when_summarized_then_should_name_the_mode_and_codec() {
        let summary = Settings::defaults(Mode::Compare).summary();
        assert_eq!(summary[0], ("mode", "compare".to_owned()));
        assert_eq!(summary[2], ("codec", "json".to_owned()));
    }
}
