mod truck;

pub use truck::{Field, TruckState};

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use strum::{Display, EnumIter, EnumString};
use thiserror::Error;

const MAX_BATTERY_PERCENT: u8 = 100;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ContractVersion(pub u16);

impl ContractVersion {
    pub const CURRENT: Self = Self(1);
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct TruckId(String);

impl TruckId {
    /// `FR-001` for 1: the fleet numbers its trucks from one.
    pub fn numbered(number: u32) -> Self {
        Self(format!("FR-{number:03}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct TripId(pub u64);

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct WindowId(pub u64);

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct Sequence(pub u64);

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct LogicalTime(pub u64);

/// A temperature in tenths of a degree Celsius, so `-185` is `-18.5 C`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct DeciCelsius(pub i32);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct BatteryPercent(u8);

impl BatteryPercent {
    pub fn new(value: u8) -> Result<Self, DomainError> {
        if value > MAX_BATTERY_PERCENT {
            return Err(DomainError::OutOfRange {
                what: "battery percent",
                value: i64::from(value),
            });
        }
        Ok(Self(value))
    }

    pub fn get(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum DomainError {
    #[error("{what} {value} is out of range")]
    OutOfRange { what: &'static str, value: i64 },
    #[error("truck id '{0}' must look like FR-042")]
    MalformedTruck(String),
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
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Region {
    North,
    South,
    East,
    West,
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
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Cargo {
    Pharma,
    Frozen,
    Chilled,
    Empty,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Display, EnumIter, EnumString, Eq, Hash, PartialEq, Serialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum TemperatureBand {
    Safe,
    Warning,
    Unsafe,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Display, EnumIter, EnumString, Eq, Hash, PartialEq, Serialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum UnitState {
    Running,
    Fault,
    Offline,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Display, EnumIter, EnumString, Eq, Hash, PartialEq, Serialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum TripStatus {
    Loading,
    EnRoute,
    Delivered,
    Returning,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Display, EnumIter, EnumString, Eq, Hash, PartialEq, Serialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Operation {
    Insert,
    Update,
    Delete,
}

impl fmt::Display for TruckId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for DeciCelsius {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let magnitude = self.0.unsigned_abs();
        write!(f, "{sign}{}.{} C", magnitude / 10, magnitude % 10)
    }
}

impl fmt::Display for WindowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for TruckId {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let digits = value.strip_prefix("FR-").unwrap_or_default();
        if digits.len() >= 3 && digits.bytes().all(|byte| byte.is_ascii_digit()) {
            Ok(Self(value.to_owned()))
        } else {
            Err(DomainError::MalformedTruck(value.to_owned()))
        }
    }
}

impl TryFrom<u8> for BatteryPercent {
    type Error = DomainError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<BatteryPercent> for u8 {
    fn from(value: BatteryPercent) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strum::IntoEnumIterator;

    #[test]
    fn given_a_battery_above_one_hundred_when_built_then_should_fail() {
        assert_eq!(
            BatteryPercent::new(101),
            Err(DomainError::OutOfRange {
                what: "battery percent",
                value: 101
            })
        );
        assert_eq!(BatteryPercent::new(100).expect("in range").get(), 100);
    }

    #[test]
    fn given_temperatures_when_rendered_then_should_show_one_decimal_and_the_sign() {
        assert_eq!(DeciCelsius(92).to_string(), "9.2 C");
        assert_eq!(DeciCelsius(-185).to_string(), "-18.5 C");
        assert_eq!(DeciCelsius(-5).to_string(), "-0.5 C");
    }

    #[test]
    fn given_a_truck_number_when_formatted_then_should_pad_to_three_digits() {
        assert_eq!(TruckId::numbered(42).as_str(), "FR-042");
        assert_eq!(TruckId::numbered(800).as_str(), "FR-800");
        assert_eq!(
            "FR-042".parse::<TruckId>().expect("valid id"),
            TruckId::numbered(42)
        );
        assert_eq!(
            "TRUCK-1".parse::<TruckId>(),
            Err(DomainError::MalformedTruck("TRUCK-1".to_owned()))
        );
    }

    #[test]
    fn given_every_category_when_rendered_and_parsed_then_should_round_trip() {
        for region in Region::iter() {
            assert_eq!(
                region.to_string().parse::<Region>().expect("region"),
                region
            );
        }
        for cargo in Cargo::iter() {
            assert_eq!(cargo.to_string().parse::<Cargo>().expect("cargo"), cargo);
        }
        assert_eq!(TripStatus::EnRoute.to_string(), "en_route");
        assert_eq!(
            serde_json::to_value(TemperatureBand::Unsafe).expect("band serializes"),
            serde_json::json!("unsafe")
        );
    }
}
