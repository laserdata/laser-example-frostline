use crate::domain::{
    BatteryPercent, Cargo, DeciCelsius, LogicalTime, Region, Sequence, TemperatureBand, TripStatus,
    TruckId, TruckState, UnitState, WindowId,
};
use crate::event::{Change, EventHeader, FleetEvent, Telemetry};
use crate::names::RunId;

/// Scenario events the walkthrough tests filters against, and unit tests reuse.
pub const RUN: &str = "0badc0de";

/// The truck every scenario follows: FR-042 hauling pharma in the north.
pub fn truck() -> TruckId {
    TruckId::numbered(42)
}

pub fn header(event_id: u64) -> EventHeader {
    EventHeader {
        run_id: RUN.parse::<RunId>().expect("fixture run id is valid"),
        event_id,
        window_id: WindowId(0),
        sequence: Sequence(event_id),
        logical_time: LogicalTime(event_id * 1_000),
    }
}

pub fn state(region: Region, cargo: Cargo, temperature: i32, battery: u8) -> TruckState {
    let temperature = DeciCelsius(temperature);
    TruckState {
        temperature_band: TemperatureBand::for_temperature(temperature, cargo),
        temperature_deci_c: temperature,
        battery_pct: BatteryPercent::new(battery).expect("fixture battery is valid"),
        unit_state: UnitState::Running,
        trip_status: TripStatus::EnRoute,
        region,
        cargo,
        declared_weight_tonnes: "12.50".to_owned(),
    }
}

/// FR-042 warms from a safe 5.0 C to an unsafe 12.0 C.
pub fn enters_unsafe() -> FleetEvent {
    let before = state(Region::North, Cargo::Pharma, 50, 80);
    let after = state(Region::North, Cargo::Pharma, 120, 79);
    FleetEvent::change(header(1), &truck(), Change::update(before, after))
}

/// A battery reading of a truck that is already unsafe: no new transition.
pub fn battery_while_unsafe() -> FleetEvent {
    let before = state(Region::North, Cargo::Pharma, 120, 79);
    let after = state(Region::North, Cargo::Pharma, 120, 78);
    FleetEvent::change(header(2), &truck(), Change::update(before, after))
}

pub fn routine_update(region: Region, cargo: Cargo) -> FleetEvent {
    let setpoint = cargo.setpoint().0;
    let before = state(region, cargo, setpoint, 80);
    let after = state(region, cargo, setpoint, 79);
    FleetEvent::change(header(3), &truck(), Change::update(before, after))
}

pub fn retired(cargo: Cargo) -> FleetEvent {
    let before = state(Region::North, cargo, cargo.setpoint().0, 60);
    FleetEvent::change(header(4), &truck(), Change::delete(before))
}

pub fn reefer_fault() -> FleetEvent {
    let state = state(Region::North, Cargo::Pharma, 90, 70);
    let telemetry = Telemetry {
        temperature_deci_c: state.temperature_deci_c,
        battery_pct: state.battery_pct,
        unit_state: UnitState::Fault,
        fault_code: Some(17),
        latitude_e6: 64_000_000,
        longitude_e6: 25_000_000,
        samples: vec![88, 89, 90],
    };
    FleetEvent::telemetry(header(5), &truck(), &state, telemetry)
}

pub fn checkpoint(partition_id: u32) -> FleetEvent {
    FleetEvent::checkpoint(header(6), partition_id, false)
}
