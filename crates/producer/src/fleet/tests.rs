use super::*;
use frostline_shared::domain::{LogicalTime, Operation, Sequence, WindowId};
use frostline_shared::event::EventKind;
use frostline_shared::{Mode, Settings};

#[test]
fn given_the_same_seed_when_generated_twice_then_should_give_the_same_events() {
    let first = events(&settings(1312), 1000);
    let second = events(&settings(1312), 1000);
    assert_eq!(first, second);
    assert_ne!(first, events(&settings(1313), 1000));
}

#[test]
fn given_the_default_settings_when_built_then_should_hold_every_truck() {
    let fleet = Fleet::new(&settings(1312));
    assert_eq!(fleet.trucks().len(), 800);
    assert_eq!(fleet.trucks()[41].id.as_str(), "FR-042");
}

#[test]
fn given_many_steps_when_generated_then_should_validate_every_event() {
    for event in events(&settings(5), 20_000) {
        event.validate().expect("a generated event is valid");
    }
}

#[test]
fn given_many_steps_when_counted_then_should_follow_the_setting_the_change_mix() {
    let generated = events(&settings(11), 10_000);
    let changes = generated
        .iter()
        .filter(|event| event.kind == EventKind::Change)
        .count();
    assert!((7_800..=8_200).contains(&changes), "{changes} changes");
}

#[test]
fn given_a_retired_truck_when_generated_then_should_join_next_its_replacement() {
    let generated = events(&settings(3), 60_000);
    let delete = generated
        .iter()
        .position(|event| {
            event
                .change
                .as_ref()
                .is_some_and(|change| change.op == Operation::Delete)
        })
        .expect("a retirement happens in sixty thousand steps");
    let next = generated[delete + 1]
        .change
        .as_ref()
        .expect("a change follows");
    assert_eq!(next.op, Operation::Insert);
}

#[test]
fn given_incidents_when_generated_then_should_enter_and_leave_the_unsafe_band_trucks() {
    let generated = events(
        &Settings {
            incident_onset_per_mille: 50,
            ..settings(21)
        },
        20_000,
    );
    let entered = generated
        .iter()
        .filter(|event| {
            event
                .change
                .as_ref()
                .is_some_and(|change| change.enters_unsafe())
        })
        .count();
    assert!(entered > 10, "{entered} transitions");
}

fn settings(seed: u64) -> Settings {
    Settings {
        seed,
        ..Settings::defaults(Mode::Finite)
    }
}

fn events(settings: &Settings, count: u64) -> Vec<FleetEvent> {
    let mut fleet = Fleet::new(settings);
    (0..count)
        .map(|event_id| {
            fleet.next_step().into_event(EventHeader {
                run_id: "0badc0de".parse().expect("run id"),
                event_id,
                window_id: WindowId(0),
                sequence: Sequence(event_id),
                logical_time: LogicalTime(event_id),
            })
        })
        .collect()
}
