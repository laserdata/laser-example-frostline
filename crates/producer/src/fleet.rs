use crate::incident::{Incidents, Phase};
use crate::partition::partition_for;
use frostline_shared::Settings;
use frostline_shared::domain::{
    BatteryPercent, Cargo, DeciCelsius, Region, TemperatureBand, TripStatus, TruckId, TruckState,
    UnitState,
};
use frostline_shared::event::{Change, EventHeader, FleetEvent, Telemetry};
use rand::rngs::ChaCha8Rng;
use rand::{RngExt, SeedableRng};
use std::collections::VecDeque;

pub const GENERATOR_VERSION: &str = "1";
const RETIRE_ONE_IN: u32 = 2000;
const FAULT_CODE: u16 = 17;
const REGIONS: [Region; 4] = [Region::North, Region::South, Region::East, Region::West];
const CARGOS: [Cargo; 4] = [Cargo::Pharma, Cargo::Frozen, Cargo::Chilled, Cargo::Empty];

/// The simulated carrier. The same seed and settings give the same sequence of events.
pub struct Fleet {
    trucks: Vec<Truck>,
    rng: ChaCha8Rng,
    incidents: Incidents,
    pending: VecDeque<Step>,
    next_number: u32,
    partitions: u32,
    change_percent: u8,
    samples: u16,
}

#[derive(Clone, Debug)]
pub struct Truck {
    pub id: TruckId,
    pub state: TruckState,
    pub partition: u32,
    latitude_e6: i32,
    longitude_e6: i32,
}

/// One generated fact before the producer gives it an envelope.
#[derive(Clone, Debug)]
pub struct Step {
    pub partition: u32,
    truck: TruckId,
    body: Body,
}

#[derive(Clone, Debug)]
enum Body {
    Change(Change),
    Telemetry(TruckState, Telemetry),
}

impl Fleet {
    pub fn new(settings: &Settings) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(settings.seed);
        let trucks = (1..=settings.fleet_size)
            .map(|number| fresh_truck(&mut rng, number, settings.partitions))
            .collect();
        Self {
            trucks,
            rng,
            incidents: Incidents::new(
                settings.incident_onset_per_mille,
                settings.incident_min_updates..=settings.incident_max_updates,
            ),
            pending: VecDeque::new(),
            next_number: settings.fleet_size + 1,
            partitions: settings.partitions,
            change_percent: settings.change_percent,
            samples: settings.diagnostics_samples,
        }
    }

    pub fn next_step(&mut self) -> Step {
        if let Some(step) = self.pending.pop_front() {
            return step;
        }
        let index = self.rng.random_range(0..self.trucks.len());
        if self.rng.random_range(0..100) >= self.change_percent {
            return self.telemetry(index);
        }
        if self.rng.random_ratio(1, RETIRE_ONE_IN) {
            return self.retire(index);
        }
        self.update(index)
    }

    pub fn trucks(&self) -> &[Truck] {
        &self.trucks
    }

    fn update(&mut self, index: usize) -> Step {
        let phase = self.incidents.tick(&mut self.rng, &self.trucks[index]);
        let truck = &mut self.trucks[index];
        let before = truck.state.clone();
        let cargo = truck.state.cargo;
        let target = match phase {
            Phase::Unsafe => cargo.setpoint().0 + unsafe_offset(cargo),
            Phase::Normal | Phase::Recovering => cargo.setpoint().0,
        };
        let jitter = [-2, -1, 1, 2][self.rng.random_range(0..4)];
        let temperature = DeciCelsius(target + jitter);
        let state = &mut truck.state;
        state.temperature_deci_c = temperature;
        state.temperature_band = TemperatureBand::for_temperature(temperature, cargo);
        state.unit_state = match phase {
            Phase::Unsafe => UnitState::Fault,
            Phase::Normal | Phase::Recovering => UnitState::Running,
        };
        if self.rng.random_ratio(1, 3) {
            let drained = state.battery_pct.get().saturating_sub(1).max(5);
            state.battery_pct =
                BatteryPercent::new(drained).expect("a drained battery stays in range");
        }
        if self.rng.random_ratio(1, 40) {
            state.trip_status = next_trip(state.trip_status);
            if state.trip_status == TripStatus::Loading {
                state.declared_weight_tonnes = weight(&mut self.rng);
                state.battery_pct = BatteryPercent::new(100).expect("a full battery is in range");
            }
        }
        // An update always changes something, so a repeat reading nudges the temperature once more.
        if truck.state == before {
            let nudged = DeciCelsius(truck.state.temperature_deci_c.0 + 1);
            truck.state.temperature_deci_c = nudged;
            truck.state.temperature_band = TemperatureBand::for_temperature(nudged, cargo);
        }
        Step {
            partition: truck.partition,
            truck: truck.id.clone(),
            body: Body::Change(Change::update(before, truck.state.clone())),
        }
    }

    fn telemetry(&mut self, index: usize) -> Step {
        let truck = &mut self.trucks[index];
        truck.latitude_e6 += self.rng.random_range(-500..=500);
        truck.longitude_e6 += self.rng.random_range(-500..=500);
        let base = i16::try_from(truck.state.temperature_deci_c.0).unwrap_or(i16::MAX);
        let telemetry = Telemetry {
            temperature_deci_c: truck.state.temperature_deci_c,
            battery_pct: truck.state.battery_pct,
            unit_state: truck.state.unit_state,
            fault_code: (truck.state.unit_state == UnitState::Fault).then_some(FAULT_CODE),
            latitude_e6: truck.latitude_e6,
            longitude_e6: truck.longitude_e6,
            samples: (0..self.samples)
                .map(|_| base.saturating_add(self.rng.random_range(-3..=3)))
                .collect(),
        };
        Step {
            partition: truck.partition,
            truck: truck.id.clone(),
            body: Body::Telemetry(truck.state.clone(), telemetry),
        }
    }

    // A retired truck leaves with a delete, and its replacement joins with an insert on the next step.
    fn retire(&mut self, index: usize) -> Step {
        let replacement = fresh_truck(&mut self.rng, self.next_number, self.partitions);
        self.next_number += 1;
        let retired = std::mem::replace(&mut self.trucks[index], replacement.clone());
        self.incidents.forget(&retired.id);
        self.pending.push_back(Step {
            partition: replacement.partition,
            truck: replacement.id.clone(),
            body: Body::Change(Change::insert(replacement.state.clone())),
        });
        Step {
            partition: retired.partition,
            truck: retired.id,
            body: Body::Change(Change::delete(retired.state)),
        }
    }
}

impl Step {
    pub fn into_event(self, header: EventHeader) -> FleetEvent {
        match self.body {
            Body::Change(change) => FleetEvent::change(header, &self.truck, change),
            Body::Telemetry(state, telemetry) => {
                FleetEvent::telemetry(header, &self.truck, &state, telemetry)
            }
        }
    }
}

fn fresh_truck(rng: &mut ChaCha8Rng, number: u32, partitions: u32) -> Truck {
    let id = TruckId::numbered(number);
    let region = REGIONS[rng.random_range(0..REGIONS.len())];
    let cargo = CARGOS[rng.random_range(0..CARGOS.len())];
    let temperature = cargo.setpoint();
    Truck {
        partition: partition_for(&id, partitions),
        id,
        state: TruckState {
            temperature_band: TemperatureBand::for_temperature(temperature, cargo),
            temperature_deci_c: temperature,
            battery_pct: BatteryPercent::new(rng.random_range(40..=100))
                .expect("a start battery is in range"),
            unit_state: UnitState::Running,
            trip_status: TripStatus::Loading,
            region,
            cargo,
            declared_weight_tonnes: weight(rng),
        },
        latitude_e6: rng.random_range(45_000_000..70_000_000),
        longitude_e6: rng.random_range(5_000_000..30_000_000),
    }
}

fn weight(rng: &mut ChaCha8Rng) -> String {
    format!(
        "{}.{:02}",
        rng.random_range(4..=16),
        rng.random_range(0..100)
    )
}

// How far above its setpoint a failing unit lets each cargo warm, always into the unsafe band.
fn unsafe_offset(cargo: Cargo) -> i32 {
    match cargo {
        Cargo::Frozen => 100,
        Cargo::Pharma => 70,
        Cargo::Chilled => 60,
        Cargo::Empty => 0,
    }
}

fn next_trip(status: TripStatus) -> TripStatus {
    match status {
        TripStatus::Loading => TripStatus::EnRoute,
        TripStatus::EnRoute => TripStatus::Delivered,
        TripStatus::Delivered => TripStatus::Returning,
        TripStatus::Returning => TripStatus::Loading,
    }
}

#[cfg(test)]
mod tests;
