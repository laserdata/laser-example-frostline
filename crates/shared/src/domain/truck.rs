use super::{BatteryPercent, Cargo, DeciCelsius, Region, TemperatureBand, TripStatus, UnitState};
use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TruckState {
    pub temperature_band: TemperatureBand,
    pub temperature_deci_c: DeciCelsius,
    pub battery_pct: BatteryPercent,
    pub unit_state: UnitState,
    pub trip_status: TripStatus,
    pub region: Region,
    pub cargo: Cargo,
    /// A decimal string, so a filter has to coerce it before comparing.
    pub declared_weight_tonnes: String,
}

impl TruckState {
    /// The changed fields in declaration order, the CDC evidence a filter reads.
    pub fn diff(&self, other: &Self) -> Vec<Field> {
        [
            (
                self.temperature_band != other.temperature_band,
                Field::TemperatureBand,
            ),
            (
                self.temperature_deci_c != other.temperature_deci_c,
                Field::TemperatureDeciC,
            ),
            (self.battery_pct != other.battery_pct, Field::BatteryPct),
            (self.unit_state != other.unit_state, Field::UnitState),
            (self.trip_status != other.trip_status, Field::TripStatus),
            (self.region != other.region, Field::Region),
            (self.cargo != other.cargo, Field::Cargo),
            (
                self.declared_weight_tonnes != other.declared_weight_tonnes,
                Field::DeclaredWeightTonnes,
            ),
        ]
        .into_iter()
        .filter_map(|(changed, field)| changed.then_some(field))
        .collect()
    }
}

#[derive(
    Clone, Copy, Debug, Deserialize, Display, EnumIter, EnumString, Eq, Hash, PartialEq, Serialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Field {
    TemperatureBand,
    TemperatureDeciC,
    BatteryPct,
    UnitState,
    TripStatus,
    Region,
    Cargo,
    DeclaredWeightTonnes,
}

impl TemperatureBand {
    /// Frozen goods stay at or below -15 C, pharma between 2 and 8 C, chilled between 0 and 5 C.
    pub fn for_temperature(temperature: DeciCelsius, cargo: Cargo) -> Self {
        let value = temperature.0;
        let (safe, warning) = match cargo {
            Cargo::Frozen => (i32::MIN..=-150, i32::MIN..=-100),
            Cargo::Pharma => (20..=80, 0..=100),
            Cargo::Chilled => (0..=50, -20..=80),
            Cargo::Empty => return TemperatureBand::Safe,
        };
        if safe.contains(&value) {
            TemperatureBand::Safe
        } else if warning.contains(&value) {
            TemperatureBand::Warning
        } else {
            TemperatureBand::Unsafe
        }
    }
}

impl Cargo {
    /// The temperature the refrigeration unit holds for this cargo.
    pub fn setpoint(&self) -> DeciCelsius {
        DeciCelsius(match self {
            Cargo::Frozen => -180,
            Cargo::Pharma => 50,
            Cargo::Chilled => 30,
            Cargo::Empty => 150,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_equal_states_when_diffed_then_should_report_nothing() {
        assert!(state().diff(&state()).is_empty());
    }

    #[test]
    fn given_changed_fields_when_diffed_then_should_list_them_in_declaration_order() {
        let mut after = state();
        after.declared_weight_tonnes = "9.00".to_owned();
        after.temperature_band = TemperatureBand::Unsafe;
        after.battery_pct = BatteryPercent::new(40).expect("valid battery");
        assert_eq!(
            state().diff(&after),
            [
                Field::TemperatureBand,
                Field::BatteryPct,
                Field::DeclaredWeightTonnes
            ]
        );
    }

    #[test]
    fn given_band_boundaries_when_classified_then_should_follow_the_cargo_thresholds() {
        let cases = [
            (Cargo::Frozen, -150, TemperatureBand::Safe),
            (Cargo::Frozen, -149, TemperatureBand::Warning),
            (Cargo::Frozen, -99, TemperatureBand::Unsafe),
            (Cargo::Pharma, 20, TemperatureBand::Safe),
            (Cargo::Pharma, 81, TemperatureBand::Warning),
            (Cargo::Pharma, 101, TemperatureBand::Unsafe),
            (Cargo::Pharma, -1, TemperatureBand::Unsafe),
            (Cargo::Chilled, 50, TemperatureBand::Safe),
            (Cargo::Chilled, 81, TemperatureBand::Unsafe),
            (Cargo::Empty, 400, TemperatureBand::Safe),
        ];
        for (cargo, value, band) in cases {
            assert_eq!(
                TemperatureBand::for_temperature(DeciCelsius(value), cargo),
                band,
                "{cargo} at {value}"
            );
        }
    }

    #[test]
    fn given_every_cargo_when_held_at_its_setpoint_then_should_be_safe() {
        for cargo in [Cargo::Frozen, Cargo::Pharma, Cargo::Chilled, Cargo::Empty] {
            assert_eq!(
                TemperatureBand::for_temperature(cargo.setpoint(), cargo),
                TemperatureBand::Safe
            );
        }
    }

    fn state() -> TruckState {
        TruckState {
            temperature_band: TemperatureBand::Safe,
            temperature_deci_c: DeciCelsius(50),
            battery_pct: BatteryPercent::new(80).expect("valid battery"),
            unit_state: UnitState::Running,
            trip_status: TripStatus::EnRoute,
            region: Region::North,
            cargo: Cargo::Pharma,
            declared_weight_tonnes: "12.50".to_owned(),
        }
    }
}
