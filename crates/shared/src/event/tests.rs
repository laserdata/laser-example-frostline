use super::*;
use crate::fixtures;
use serde_json::json;

#[test]
fn given_scenario_events_when_validated_then_should_pass() {
    for event in [
        fixtures::enters_unsafe(),
        fixtures::battery_while_unsafe(),
        fixtures::retired(Cargo::Frozen),
        fixtures::reefer_fault(),
        fixtures::checkpoint(2),
    ] {
        event.validate().expect("a scenario event is valid");
    }
}

#[test]
fn given_a_change_without_its_body_when_validated_then_should_report_the_mismatch() {
    let mut event = fixtures::enters_unsafe();
    event.change = None;
    assert_eq!(
        event.validate(),
        Err(EventError::BodyMismatch {
            kind: EventKind::Change
        })
    );
}

#[test]
fn given_changed_fields_that_disagree_when_validated_then_should_report_both_lists() {
    let mut event = fixtures::enters_unsafe();
    let change = event.change.as_mut().expect("change body");
    change.changed = vec![Field::Cargo];
    assert_eq!(
        event.validate(),
        Err(EventError::ChangedDisagrees {
            expected: vec![
                Field::TemperatureBand,
                Field::TemperatureDeciC,
                Field::BatteryPct
            ],
            got: vec![Field::Cargo]
        })
    );
}

#[test]
fn given_an_update_that_changes_nothing_when_validated_then_should_fail() {
    let state = fixtures::state(Region::North, Cargo::Pharma, 50, 80);
    let event = FleetEvent::change(
        fixtures::header(9),
        &fixtures::truck(),
        Change::update(state.clone(), state),
    );
    assert_eq!(event.validate(), Err(EventError::EmptyChanged));
}

#[test]
fn given_a_delete_with_changed_fields_when_validated_then_should_fail() {
    let mut event = fixtures::retired(Cargo::Pharma);
    event.change.as_mut().expect("change body").changed = vec![Field::Region];
    assert_eq!(
        event.validate(),
        Err(EventError::ChangedOnNonUpdate {
            op: Operation::Delete
        })
    );
}

#[test]
fn given_an_insert_with_a_before_state_when_validated_then_should_report_the_shape() {
    let mut event = fixtures::retired(Cargo::Pharma);
    let change = event.change.as_mut().expect("change body");
    change.op = Operation::Insert;
    assert_eq!(
        event.validate(),
        Err(EventError::ChangeShape {
            op: Operation::Insert
        })
    );
}

#[test]
fn given_a_checkpoint_with_a_truck_when_validated_then_should_fail() {
    let mut event = fixtures::checkpoint(0);
    event.truck_id = Some(fixtures::truck());
    assert_eq!(event.validate(), Err(EventError::CheckpointWithTruck));
}

#[test]
fn given_events_when_diagnosed_then_should_pick_the_unit_and_severity() {
    assert_eq!(
        fixtures::reefer_fault().diagnosis(),
        (Unit::Reefer, Severity::Error)
    );
    assert_eq!(
        fixtures::enters_unsafe().diagnosis(),
        (Unit::Reefer, Severity::Critical)
    );
    assert_eq!(
        fixtures::battery_while_unsafe().diagnosis(),
        (Unit::Engine, Severity::Info)
    );
    let mut low = fixtures::reefer_fault();
    let telemetry = low.telemetry.as_mut().expect("telemetry body");
    telemetry.fault_code = None;
    telemetry.battery_pct = BatteryPercent::new(9).expect("valid battery");
    assert_eq!(low.diagnosis(), (Unit::Battery, Severity::Warning));
}

#[test]
fn given_a_checkpoint_when_serialized_then_should_match_the_wire_shape() {
    let value = serde_json::to_value(fixtures::checkpoint(3)).expect("checkpoint serializes");
    assert_eq!(
        value,
        json!({
            "v": 1,
            "run_id": "0badc0de",
            "event_id": 6,
            "window_id": 0,
            "sequence": 6,
            "logical_time_micros": 6000,
            "truck_id": null,
            "region": null,
            "cargo": null,
            "kind": "checkpoint",
            "change": null,
            "telemetry": null,
            "checkpoint": { "window_id": 0, "partition_id": 3, "last": false }
        })
    );
}

#[test]
fn given_a_change_when_serialized_and_parsed_then_should_round_trip() {
    let event = fixtures::enters_unsafe();
    let bytes = serde_json::to_vec(&event).expect("change serializes");
    let parsed: FleetEvent = serde_json::from_slice(&bytes).expect("change parses");
    assert_eq!(parsed, event);
    assert_eq!(
        parsed.change.expect("change body").changed,
        [
            Field::TemperatureBand,
            Field::TemperatureDeciC,
            Field::BatteryPct
        ]
    );
}
