use laser_sdk::iggy::prelude::HeaderKey;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use strum::{Display, EnumIter, EnumString};
use thiserror::Error;

pub const STREAM_PREFIX: &str = "frostline";
pub const SAFETY_CURRENT_GROUP: &str = "food-safety-current";
const RUN_ID_LENGTH: usize = 8;

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct RunId(String);

impl RunId {
    /// A fresh 32-bit random id. Provisioning still checks any existing source.
    pub fn mint() -> Self {
        Self(format!("{:08x}", rand::random::<u32>()))
    }

    pub fn stream(&self) -> String {
        format!("{STREAM_PREFIX}-{}", self.0)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum RunIdError {
    #[error("run id '{0}' must be eight lowercase hex characters")]
    Malformed(String),
}

#[derive(Clone, Copy, Debug, Display, EnumIter, EnumString, Eq, PartialEq)]
#[strum(serialize_all = "snake_case")]
pub enum RunTopic {
    Changes,
    Reports,
}

#[derive(
    Clone,
    Copy,
    Debug,
    Deserialize,
    Display,
    EnumIter,
    EnumString,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum Role {
    FoodSafety,
    Maintenance,
    Regional,
}

impl Role {
    pub fn group(&self) -> &'static str {
        match self {
            Role::FoodSafety => "food-safety",
            Role::Maintenance => "maintenance",
            Role::Regional => "north-pharma",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Role::FoodSafety => "Food safety",
            Role::Maintenance => "Maintenance",
            Role::Regional => "North pharma",
        }
    }
}

#[derive(Clone, Copy, Debug, EnumIter, Eq, PartialEq)]
pub enum Header {
    Frame,
    Unit,
    Severity,
}

impl Header {
    pub fn name(&self) -> &'static str {
        match self {
            Header::Frame => "frostline.frame",
            Header::Unit => "frostline.unit",
            Header::Severity => "frostline.severity",
        }
    }

    pub fn key(&self) -> HeaderKey {
        HeaderKey::from_str(self.name()).expect("a Frostline header name is a valid key")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Frame {
    Business = 0,
    Checkpoint = 1,
}

impl Frame {
    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Frame::Business),
            1 => Some(Frame::Checkpoint),
            _ => None,
        }
    }
}

#[derive(
    Clone, Copy, Debug, Deserialize, Display, EnumIter, EnumString, Eq, Hash, PartialEq, Serialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Unit {
    Reefer,
    Battery,
    Door,
    Engine,
}

#[derive(
    Clone,
    Copy,
    Debug,
    Deserialize,
    Display,
    EnumIter,
    EnumString,
    Eq,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Severity {
    Info = 0,
    Warning = 1,
    Error = 2,
    Critical = 3,
}

impl fmt::Display for RunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for RunId {
    type Err = RunIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let valid = value.len() == RUN_ID_LENGTH
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if valid {
            Ok(Self(value.to_owned()))
        } else {
            Err(RunIdError::Malformed(value.to_owned()))
        }
    }
}

impl TryFrom<String> for RunId {
    type Error = RunIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<RunId> for String {
    fn from(value: RunId) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strum::IntoEnumIterator;

    #[test]
    fn given_a_minted_run_id_when_parsed_then_should_round_trip() {
        let run = RunId::mint();
        assert_eq!(
            run.as_str().parse::<RunId>().expect("minted id parses"),
            run
        );
        assert_eq!(run.stream(), format!("frostline-{run}"));
    }

    #[test]
    fn given_malformed_run_ids_when_parsed_then_should_fail() {
        for value in ["", "abc", "ABCDEF12", "abcdefgh", "abcdef123"] {
            assert_eq!(
                value.parse::<RunId>(),
                Err(RunIdError::Malformed(value.to_owned()))
            );
        }
    }

    #[test]
    fn given_every_role_when_rendered_and_parsed_then_should_round_trip() {
        for role in Role::iter() {
            assert_eq!(role.to_string().parse::<Role>().expect("role parses"), role);
        }
        assert_eq!(Role::FoodSafety.to_string(), "food-safety");
        assert_eq!(Role::Regional.group(), "north-pharma");
    }

    #[test]
    fn given_headers_when_named_then_should_use_the_documented_keys() {
        let names: Vec<&str> = Header::iter().map(|header| header.name()).collect();
        assert_eq!(
            names,
            ["frostline.frame", "frostline.unit", "frostline.severity"]
        );
        for header in Header::iter() {
            assert_eq!(header.key().as_str().expect("text key"), header.name());
        }
    }

    #[test]
    fn given_frame_codes_when_decoded_then_should_map_known_values_only() {
        assert_eq!(Frame::from_code(0), Some(Frame::Business));
        assert_eq!(Frame::from_code(1), Some(Frame::Checkpoint));
        assert_eq!(Frame::from_code(2), None);
    }

    #[test]
    fn given_units_and_severities_when_rendered_and_parsed_then_should_round_trip() {
        for unit in Unit::iter() {
            assert_eq!(unit.to_string().parse::<Unit>().expect("unit parses"), unit);
        }
        for severity in Severity::iter() {
            assert_eq!(
                severity
                    .to_string()
                    .parse::<Severity>()
                    .expect("severity parses"),
                severity
            );
        }
        assert!(Severity::Critical > Severity::Error);
    }
}
