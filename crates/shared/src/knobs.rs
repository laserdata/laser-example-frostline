use std::env::{self, VarError};
use std::fmt::Display;
use std::str::FromStr;
use std::time::Duration;
use thiserror::Error;

pub const CATALOG: &str = "FROSTLINE_CATALOG";
pub const CODEC: &str = "FROSTLINE_CODEC";
pub const SEED: &str = "FROSTLINE_SEED";
pub const FLEET_SIZE: &str = "FROSTLINE_FLEET_SIZE";
pub const PARTITIONS: &str = "FROSTLINE_PARTITIONS";
pub const RATE_CATCH_UP_RECORDS: &str = "FROSTLINE_RATE_CATCH_UP_RECORDS";
pub const RATE_PER_SECOND: &str = "FROSTLINE_RATE_PER_SECOND";
pub const TOTAL_RECORDS: &str = "FROSTLINE_TOTAL_RECORDS";
pub const DURATION_SECONDS: &str = "FROSTLINE_DURATION_SECONDS";
pub const CHECKPOINT_RECORDS: &str = "FROSTLINE_CHECKPOINT_RECORDS";
pub const CHANGE_PERCENT: &str = "FROSTLINE_CHANGE_PERCENT";
pub const INCIDENT_ONSET_PER_MILLE: &str = "FROSTLINE_INCIDENT_ONSET_PER_MILLE";
pub const INCIDENT_UPDATES: &str = "FROSTLINE_INCIDENT_UPDATES";
pub const DIAGNOSTICS_SAMPLES: &str = "FROSTLINE_DIAGNOSTICS_SAMPLES";
pub const BATCH_RECORDS: &str = "FROSTLINE_BATCH_RECORDS";
pub const BATCH_BYTES: &str = "FROSTLINE_BATCH_BYTES";
pub const BATCH_LINGER_MS: &str = "FROSTLINE_BATCH_LINGER_MS";
pub const PUBLISH_QUEUE_RECORDS: &str = "FROSTLINE_PUBLISH_QUEUE_RECORDS";
pub const POLL_RECORDS: &str = "FROSTLINE_POLL_RECORDS";
pub const REPLY_BYTES: &str = "FROSTLINE_REPLY_BYTES";
pub const IDLE_INTERVAL_MS: &str = "FROSTLINE_IDLE_INTERVAL_MS";
pub const WORKERS_PER_ROLE: &str = "FROSTLINE_WORKERS_PER_ROLE";
pub const LOCAL_GUARD: &str = "FROSTLINE_LOCAL_GUARD";
pub const MAX_PENDING_WINDOWS: &str = "FROSTLINE_MAX_PENDING_WINDOWS";
pub const BOARD_INTERVAL_SECONDS: &str = "FROSTLINE_BOARD_INTERVAL_SECONDS";
pub const SAMPLED_EVENTS: &str = "FROSTLINE_SAMPLED_EVENTS_PER_ROLE_PER_SECOND";
pub const CHANGES_EXPIRY_SECONDS: &str = "FROSTLINE_CHANGES_EXPIRY_SECONDS";
pub const REPORTS_EXPIRY_SECONDS: &str = "FROSTLINE_REPORTS_EXPIRY_SECONDS";
pub const DRAIN_TIMEOUT_SECONDS: &str = "FROSTLINE_DRAIN_TIMEOUT_SECONDS";
pub const READINESS_TIMEOUT_SECONDS: &str = "FROSTLINE_READINESS_TIMEOUT_SECONDS";
pub const LOG_FORMAT: &str = "FROSTLINE_LOG_FORMAT";
pub const OUTPUT_DIRECTORY: &str = "FROSTLINE_OUTPUT_DIRECTORY";
pub const KEEP_RUN: &str = "FROSTLINE_KEEP_RUN";

/// Every setting key, in the order the operations guide lists them.
pub const ALL: [&str; 32] = [
    CATALOG,
    CODEC,
    SEED,
    FLEET_SIZE,
    PARTITIONS,
    RATE_PER_SECOND,
    TOTAL_RECORDS,
    DURATION_SECONDS,
    CHECKPOINT_RECORDS,
    CHANGE_PERCENT,
    INCIDENT_ONSET_PER_MILLE,
    INCIDENT_UPDATES,
    DIAGNOSTICS_SAMPLES,
    BATCH_RECORDS,
    BATCH_BYTES,
    BATCH_LINGER_MS,
    PUBLISH_QUEUE_RECORDS,
    POLL_RECORDS,
    REPLY_BYTES,
    IDLE_INTERVAL_MS,
    WORKERS_PER_ROLE,
    LOCAL_GUARD,
    MAX_PENDING_WINDOWS,
    BOARD_INTERVAL_SECONDS,
    SAMPLED_EVENTS,
    CHANGES_EXPIRY_SECONDS,
    REPORTS_EXPIRY_SECONDS,
    DRAIN_TIMEOUT_SECONDS,
    READINESS_TIMEOUT_SECONDS,
    LOG_FORMAT,
    OUTPUT_DIRECTORY,
    KEEP_RUN,
];

#[derive(Debug, Error, PartialEq)]
pub enum ConfigError {
    #[error(
        "LaserData Cloud needs credentials. Set LASER_TOKEN, or LASER_USERNAME and LASER_PASSWORD"
    )]
    MissingCredentials,
    #[error("{key}={value} is invalid: {reason}")]
    InvalidValue {
        key: &'static str,
        value: String,
        reason: String,
    },
}

impl ConfigError {
    pub fn invalid(key: &'static str, value: impl Into<String>, reason: impl Into<String>) -> Self {
        ConfigError::InvalidValue {
            key,
            value: value.into(),
            reason: reason.into(),
        }
    }
}

/// The settings this process was started with, as `KEY=value` pairs, so a report can be run again.
pub fn configured() -> Vec<(&'static str, String)> {
    ALL.into_iter()
        .filter_map(|key| read(key).map(|value| (key, value)))
        .collect()
}

/// A number between `min` and `max`, or `default` when the variable is unset.
pub fn ranged<T>(key: &'static str, default: T, min: T, max: T) -> Result<T, ConfigError>
where
    T: FromStr + PartialOrd + Display + Copy,
{
    parse_ranged(key, read(key), default, min, max)
}

pub fn parsed<T: FromStr>(key: &'static str, default: T) -> Result<T, ConfigError> {
    parse_value(key, read(key), default)
}

/// Seconds where zero means no limit.
pub fn optional_seconds(key: &'static str, default: u64) -> Result<Option<Duration>, ConfigError> {
    let seconds = ranged(key, default, 0, u64::MAX)?;
    Ok((seconds > 0).then(|| Duration::from_secs(seconds)))
}

pub fn flag(key: &'static str, default: bool) -> Result<bool, ConfigError> {
    match env::var(key) {
        Err(VarError::NotPresent) => Ok(default),
        Err(VarError::NotUnicode(_)) => Err(ConfigError::invalid(key, "", "not valid unicode")),
        Ok(value) => parse_flag(key, &value, default),
    }
}

pub fn read(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

pub fn parse_ranged<T>(
    key: &'static str,
    raw: Option<String>,
    default: T,
    min: T,
    max: T,
) -> Result<T, ConfigError>
where
    T: FromStr + PartialOrd + Display + Copy,
{
    let Some(raw) = raw else {
        return Ok(default);
    };
    let value: T = raw
        .parse()
        .map_err(|_| ConfigError::invalid(key, raw.clone(), "expected a number"))?;
    if value < min || value > max {
        return Err(ConfigError::invalid(
            key,
            raw,
            format!("expected {min} to {max}"),
        ));
    }
    Ok(value)
}

pub fn parse_value<T: FromStr>(
    key: &'static str,
    raw: Option<String>,
    default: T,
) -> Result<T, ConfigError> {
    match raw {
        None => Ok(default),
        Some(raw) => raw
            .parse()
            .map_err(|_| ConfigError::invalid(key, raw, "unknown value")),
    }
}

pub fn parse_flag(key: &'static str, raw: &str, default: bool) -> Result<bool, ConfigError> {
    match raw.trim() {
        "" => Ok(default),
        "0" | "false" | "no" | "off" => Ok(false),
        "1" | "true" | "yes" | "on" => Ok(true),
        other => Err(ConfigError::invalid(key, other, "expected a boolean flag")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::Codec;

    #[test]
    fn given_text_when_parsed_as_a_number_then_should_name_the_key() {
        assert_eq!(
            parse_ranged(SEED, Some("x".to_owned()), 1u32, 0, 10),
            Err(ConfigError::invalid(SEED, "x", "expected a number"))
        );
    }

    #[test]
    fn given_a_value_above_the_maximum_when_parsed_then_should_state_the_range() {
        assert_eq!(
            parse_ranged(SEED, Some("11".to_owned()), 1u32, 0, 10),
            Err(ConfigError::invalid(SEED, "11", "expected 0 to 10"))
        );
    }

    #[test]
    fn given_an_unset_value_when_parsed_then_should_use_the_default() {
        assert_eq!(parse_ranged(SEED, None, 7u32, 0, 10), Ok(7));
        assert_eq!(parse_value::<u8>(SEED, None, 3), Ok(3));
    }

    #[test]
    fn given_an_unknown_enum_value_when_parsed_then_should_fail() {
        assert_eq!(
            parse_value::<Codec>(CODEC, Some("xml".to_owned()), Codec::Json),
            Err(ConfigError::invalid(CODEC, "xml", "unknown value"))
        );
    }

    #[test]
    fn given_flag_spellings_when_parsed_then_should_map_to_booleans() {
        for (raw, expected) in [
            ("1", true),
            ("yes", true),
            ("off", false),
            ("0", false),
            (" ", true),
        ] {
            assert_eq!(parse_flag(KEEP_RUN, raw, true), Ok(expected), "{raw}");
        }
        assert_eq!(
            parse_flag(KEEP_RUN, "maybe", false),
            Err(ConfigError::invalid(
                KEEP_RUN,
                "maybe",
                "expected a boolean flag"
            ))
        );
    }
}
