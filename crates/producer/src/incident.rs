use crate::fleet::Truck;
use frostline_shared::domain::{Cargo, TruckId};
use rand::RngExt;
use rand::rngs::ChaCha8Rng;
use std::collections::BTreeMap;
use std::ops::RangeInclusive;

const PER_MILLE: u32 = 1000;

/// Refrigeration failures: an onset, a number of unsafe updates, then a recovery.
pub struct Incidents {
    active: BTreeMap<TruckId, u16>,
    onset_per_mille: u16,
    updates: RangeInclusive<u16>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Normal,
    Unsafe,
    Recovering,
}

impl Incidents {
    pub fn new(onset_per_mille: u16, updates: RangeInclusive<u16>) -> Self {
        Self {
            active: BTreeMap::new(),
            onset_per_mille,
            updates,
        }
    }

    /// The phase of `truck` for its next update. Empty trucks carry nothing to spoil.
    pub fn tick(&mut self, rng: &mut ChaCha8Rng, truck: &Truck) -> Phase {
        if let Some(remaining) = self.active.get_mut(&truck.id) {
            if *remaining == 0 {
                self.active.remove(&truck.id);
                return Phase::Recovering;
            }
            *remaining -= 1;
            return Phase::Unsafe;
        }
        if truck.state.cargo == Cargo::Empty
            || !rng.random_ratio(u32::from(self.onset_per_mille), PER_MILLE)
        {
            return Phase::Normal;
        }
        let length = rng.random_range(self.updates.clone());
        self.active
            .insert(truck.id.clone(), length.saturating_sub(1));
        Phase::Unsafe
    }

    pub fn forget(&mut self, truck: &TruckId) {
        self.active.remove(truck);
    }

    pub fn active(&self) -> usize {
        self.active.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::Fleet;
    use frostline_shared::{Mode, Settings};
    use rand::SeedableRng;

    #[test]
    fn given_a_started_incident_when_ticked_then_should_stay_unsafe_for_its_length_and_recover() {
        let mut rng = ChaCha8Rng::seed_from_u64(1);
        let mut incidents = Incidents::new(1000, 4..=4);
        let truck = loaded_truck();
        let phases: Vec<Phase> = (0..5).map(|_| incidents.tick(&mut rng, &truck)).collect();
        assert_eq!(phases[..4], [Phase::Unsafe; 4]);
        assert_eq!(phases[4], Phase::Recovering);
        assert_eq!(incidents.active(), 0);
    }

    #[test]
    fn given_a_three_per_mille_onset_when_rolled_often_then_should_start_near_that_rate() {
        let mut rng = ChaCha8Rng::seed_from_u64(9);
        let truck = loaded_truck();
        let mut started = 0;
        for _ in 0..100_000 {
            let mut incidents = Incidents::new(3, 1..=1);
            if incidents.tick(&mut rng, &truck) == Phase::Unsafe {
                started += 1;
            }
        }
        assert!((240..=360).contains(&started), "{started} onsets");
    }

    #[test]
    fn given_an_empty_truck_when_ticked_then_should_never_start_an_incident() {
        let mut rng = ChaCha8Rng::seed_from_u64(2);
        let mut incidents = Incidents::new(1000, 1..=1);
        let mut truck = loaded_truck();
        truck.state.cargo = Cargo::Empty;
        assert_eq!(incidents.tick(&mut rng, &truck), Phase::Normal);
    }

    fn loaded_truck() -> Truck {
        let settings = Settings {
            fleet_size: 8,
            ..Settings::defaults(Mode::Finite)
        };
        Fleet::new(&settings)
            .trucks()
            .iter()
            .find(|truck| truck.state.cargo != Cargo::Empty)
            .cloned()
            .expect("a loaded truck in the seeded fleet")
    }
}
