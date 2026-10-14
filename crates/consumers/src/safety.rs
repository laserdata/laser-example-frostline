use crate::handler::{BoundedTable, Delivered, Handler};
use frostline_shared::domain::{DeciCelsius, Operation, TruckId};
use frostline_shared::event::FleetEvent;

/// Food safety keeps the open incidents: trucks that entered the unsafe band.
pub struct SafetyHandler {
    label: &'static str,
    incidents: BoundedTable<TruckId, DeciCelsius>,
    transitions: u64,
    retired: u64,
}

impl SafetyHandler {
    pub fn new(label: &'static str, bound: usize) -> Self {
        Self {
            label,
            incidents: BoundedTable::new(bound),
            transitions: 0,
            retired: 0,
        }
    }
}

impl Handler for SafetyHandler {
    fn handle(&mut self, event: &FleetEvent, _delivered: Delivered) {
        let (Some(truck), Some(change)) = (&event.truck_id, &event.change) else {
            return;
        };
        match (change.op, &change.after) {
            (Operation::Delete, _) => {
                self.incidents.remove(truck);
                self.retired += 1;
            }
            (_, Some(after)) => {
                self.incidents
                    .insert(truck.clone(), after.temperature_deci_c);
                self.transitions += 1;
            }
            (_, None) => {}
        }
    }

    fn narrate(&self, event: &FleetEvent, delivered: Delivered) -> Option<String> {
        let truck = event.truck_id.as_ref()?;
        let change = event.change.as_ref()?;
        let position = format!(
            "partition {} offset {}",
            delivered.partition, delivered.offset
        );
        Some(match (&change.before, &change.after) {
            (Some(before), Some(after)) if before.temperature_band == after.temperature_band => {
                format!(
                    "Truck {truck} is still {} at {}. {} received {position}.",
                    after.temperature_band, after.temperature_deci_c, self.label
                )
            }
            (Some(before), Some(after)) => format!(
                "Truck {truck} went from {} to {} at {}. {} received {position}.",
                before.temperature_band,
                after.temperature_band,
                after.temperature_deci_c,
                self.label
            ),
            _ => format!(
                "Truck {truck} left the fleet with {} cargo. {} received {position}.",
                event.cargo?, self.label
            ),
        })
    }

    fn status(&self) -> String {
        format!(
            "{} unsafe updates handled, {} refrigerated trucks retired, {} trucks tracked",
            self.transitions,
            self.retired,
            self.incidents.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use frostline_shared::domain::Cargo;
    use frostline_shared::fixtures;

    const AT: Delivered = Delivered {
        partition: 2,
        offset: 814,
    };

    #[test]
    fn given_a_transition_and_then_a_retirement_when_handled_then_should_open_and_close_the_incident()
     {
        let mut handler = SafetyHandler::new("Food safety", 10);
        handler.handle(&fixtures::enters_unsafe(), AT);
        assert_eq!(handler.incidents.len(), 1);
        handler.handle(&fixtures::retired(Cargo::Pharma), AT);
        assert!(handler.incidents.is_empty());
    }

    #[test]
    fn given_a_transition_when_narrated_then_should_name_the_truck_band_and_position() {
        let handler = SafetyHandler::new("Food safety", 10);
        assert_eq!(
            handler
                .narrate(&fixtures::enters_unsafe(), AT)
                .expect("a line"),
            "Truck FR-042 went from safe to unsafe at 12.0 C. Food safety received partition 2 offset 814."
        );
    }
}
