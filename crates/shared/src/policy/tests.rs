use super::*;
use crate::codec::{Headers, SchemaSet};
use crate::domain::{Cargo, Region};
use crate::event::{Change, Telemetry};
use crate::fixtures;
use laser_sdk::filters::Verdict;
use laser_sdk::iggy::prelude::HeaderKind;
use laser_sdk::wire::filter::eval::{
    CompiledFilter, DecodeLimits, FilterRecord, HeaderRef, HeaderValueRef,
};
use rand::rngs::ChaCha8Rng;
use rand::{RngExt, SeedableRng};
use strum::IntoEnumIterator;

const GENERATED_EVENTS: usize = 500;

#[test]
fn given_the_unsafe_transition_when_food_safety_evaluates_then_should_select_it() {
    assert_eq!(
        verdict(Role::FoodSafety, &fixtures::enters_unsafe()),
        Verdict::Selected
    );
}

#[test]
fn given_a_battery_update_while_unsafe_when_food_safety_evaluates_then_should_reject_it() {
    let event = fixtures::battery_while_unsafe();
    assert_eq!(verdict(Role::FoodSafety, &event), Verdict::Rejected);
    assert!(RolePolicy::safety_current().matches(&event));
}

#[test]
fn given_refrigerated_trucks_leaving_when_food_safety_evaluates_then_should_select_only_those() {
    assert_eq!(
        verdict(Role::FoodSafety, &fixtures::retired(Cargo::Frozen)),
        Verdict::Selected
    );
    assert_eq!(
        verdict(Role::FoodSafety, &fixtures::retired(Cargo::Empty)),
        Verdict::Rejected
    );
}

#[test]
fn given_a_reefer_fault_when_maintenance_evaluates_headers_then_should_select_it() {
    assert_eq!(
        verdict(Role::Maintenance, &fixtures::reefer_fault()),
        Verdict::Selected
    );
    assert_eq!(
        verdict(Role::Maintenance, &fixtures::battery_while_unsafe()),
        Verdict::Rejected
    );
}

#[test]
fn given_a_severity_written_as_text_when_maintenance_evaluates_then_should_reject_it() {
    let filter = RolePolicy::for_role(Role::Maintenance).filter(Codec::Json, &[]);
    let headers = [
        HeaderRef {
            key: Header::Unit.name(),
            value: HeaderValueRef::String("reefer"),
        },
        HeaderRef {
            key: Header::Severity.name(),
            value: HeaderValueRef::String("2"),
        },
        HeaderRef {
            key: Header::Frame.name(),
            value: HeaderValueRef::Uint(0),
        },
    ];
    let compiled = CompiledFilter::compile(&filter).expect("filter compiles");
    let record = FilterRecord {
        payload: b"opaque",
        headers: &headers,
    };
    assert_eq!(
        compiled.evaluate(&record, &DecodeLimits::default()),
        Verdict::Rejected
    );
}

#[test]
fn given_heavy_north_pharma_loads_when_regional_evaluates_then_should_respect_the_coercion_boundary()
 {
    let mut heavy = fixtures::routine_update(Region::North, Cargo::Pharma);
    set_weight(&mut heavy, "10.00");
    let mut light = heavy.clone();
    set_weight(&mut light, "9.99");
    assert_eq!(verdict(Role::Regional, &heavy), Verdict::Selected);
    assert_eq!(verdict(Role::Regional, &light), Verdict::Rejected);
    let south = fixtures::routine_update(Region::South, Cargo::Pharma);
    assert_eq!(verdict(Role::Regional, &south), Verdict::Rejected);
    assert_eq!(
        verdict(Role::Regional, &fixtures::reefer_fault()),
        Verdict::Selected
    );
}

#[test]
fn given_a_checkpoint_with_an_undecodable_payload_when_any_role_evaluates_then_should_select_it() {
    let headers = [HeaderRef {
        key: Header::Frame.name(),
        value: HeaderValueRef::Uint(1),
    }];
    for role in Role::iter() {
        let filter = RolePolicy::for_role(role).filter(Codec::Json, &[]);
        let compiled = CompiledFilter::compile(&filter).expect("filter compiles");
        let record = FilterRecord {
            payload: b"not json at all",
            headers: &headers,
        };
        assert_eq!(
            compiled.evaluate(&record, &DecodeLimits::default()),
            Verdict::Selected,
            "{role}"
        );
        assert!(!RolePolicy::for_role(role).matches(&fixtures::checkpoint(0)));
    }
}

#[test]
fn given_generated_events_when_evaluated_then_should_agree_with_the_hand_written_oracle() {
    let mut rng = ChaCha8Rng::seed_from_u64(7);
    let policies: Vec<RolePolicy> = Role::iter()
        .map(RolePolicy::for_role)
        .chain([RolePolicy::safety_current()])
        .collect();
    for index in 0..GENERATED_EVENTS {
        let event = random_event(&mut rng, index as u64);
        for policy in &policies {
            let expected = if policy.matches(&event) {
                Verdict::Selected
            } else {
                Verdict::Rejected
            };
            assert_eq!(
                evaluate(policy, &event),
                expected,
                "{} on {event:?}",
                policy.role
            );
        }
    }
}

fn verdict(role: Role, event: &FleetEvent) -> Verdict {
    evaluate(&RolePolicy::for_role(role), event)
}

fn evaluate(policy: &RolePolicy, event: &FleetEvent) -> Verdict {
    let schemas = SchemaSet::default();
    let payload = Codec::Json.encode(event, &schemas).expect("event encodes");
    let headers = Codec::Json.headers(event, &schemas).expect("headers build");
    let refs = header_refs(&headers);
    let compiled =
        CompiledFilter::compile(&policy.filter(Codec::Json, &[])).expect("filter compiles");
    compiled.evaluate(
        &FilterRecord {
            payload: &payload,
            headers: &refs,
        },
        &DecodeLimits::default(),
    )
}

fn header_refs(headers: &Headers) -> Vec<HeaderRef<'_>> {
    headers
        .iter()
        .map(|(key, value)| HeaderRef {
            key: key.as_str().expect("header keys are text"),
            value: match value.kind() {
                HeaderKind::Uint8 => {
                    HeaderValueRef::Uint(u64::from(u8::try_from(value).expect("uint8")))
                }
                HeaderKind::Uint32 => {
                    HeaderValueRef::Uint(u64::from(u32::try_from(value).expect("uint32")))
                }
                HeaderKind::String => HeaderValueRef::String(value.as_str().expect("text header")),
                other => panic!("Frostline stamps no {other} header"),
            },
        })
        .collect()
}

fn random_event(rng: &mut ChaCha8Rng, event_id: u64) -> FleetEvent {
    let regions: Vec<Region> = Region::iter().collect();
    let cargos: Vec<Cargo> = Cargo::iter().collect();
    let region = regions[rng.random_range(0..regions.len())];
    let cargo = cargos[rng.random_range(0..cargos.len())];
    let before_temperature = cargo.setpoint().0 + rng.random_range(-40..=90);
    let mut before = fixtures::state(region, cargo, before_temperature, rng.random_range(5..=100));
    before.declared_weight_tonnes = format!(
        "{}.{:02}",
        rng.random_range(5..=15),
        rng.random_range(0..100)
    );
    let header = fixtures::header(event_id);
    match rng.random_range(0..10) {
        0 => FleetEvent::change(header, &fixtures::truck(), Change::delete(before)),
        1..=3 => {
            let state = before.clone();
            let telemetry = Telemetry {
                temperature_deci_c: state.temperature_deci_c,
                battery_pct: state.battery_pct,
                unit_state: state.unit_state,
                fault_code: rng.random_ratio(1, 3).then_some(17),
                latitude_e6: 60_000_000,
                longitude_e6: 20_000_000,
                samples: vec![1, 2, 3],
            };
            FleetEvent::telemetry(header, &fixtures::truck(), &state, telemetry)
        }
        _ => {
            let after_temperature = before.temperature_deci_c.0 + rng.random_range(-60..=60);
            let mut after = fixtures::state(
                region,
                cargo,
                after_temperature,
                before.battery_pct.get().saturating_sub(1).max(1),
            );
            after.declared_weight_tonnes = before.declared_weight_tonnes.clone();
            if after == before {
                after.battery_pct = fixtures::state(region, cargo, 0, 3).battery_pct;
            }
            FleetEvent::change(header, &fixtures::truck(), Change::update(before, after))
        }
    }
}

fn set_weight(event: &mut FleetEvent, weight: &str) {
    let change = event.change.as_mut().expect("change body");
    change
        .after
        .as_mut()
        .expect("after state")
        .declared_weight_tonnes = weight.to_owned();
}
