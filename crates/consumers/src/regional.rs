use crate::handler::{BoundedTable, Delivered, Handler};
use frostline_shared::domain::{DeciCelsius, TruckId};
use frostline_shared::event::FleetEvent;

/// Regional operations follows the heavy pharma and frozen loads it received in the north.
pub struct RegionalHandler {
    trucks: BoundedTable<TruckId, DeciCelsius>,
    readings: u64,
    changes: u64,
}

impl RegionalHandler {
    pub fn new(bound: usize) -> Self {
        Self {
            trucks: BoundedTable::new(bound),
            readings: 0,
            changes: 0,
        }
    }
}

impl Handler for RegionalHandler {
    fn handle(&mut self, event: &FleetEvent, _delivered: Delivered) {
        let Some(truck) = &event.truck_id else {
            return;
        };
        if let Some(telemetry) = &event.telemetry {
            self.readings += 1;
            self.trucks
                .insert(truck.clone(), telemetry.temperature_deci_c);
        } else if let Some(after) = event
            .change
            .as_ref()
            .and_then(|change| change.after.as_ref())
        {
            self.changes += 1;
            self.trucks.insert(truck.clone(), after.temperature_deci_c);
        }
    }

    fn narrate(&self, event: &FleetEvent, delivered: Delivered) -> Option<String> {
        let truck = event.truck_id.as_ref()?;
        Some(format!(
            "Truck {truck} with {} cargo is in the north slice. North pharma received partition {} offset {}.",
            event.cargo?, delivered.partition, delivered.offset
        ))
    }

    fn status(&self) -> String {
        format!(
            "{} trucks seen, {} readings and {} changes received, counted over received records only",
            self.trucks.len(),
            self.readings,
            self.changes
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use frostline_shared::fixtures;

    #[test]
    fn given_received_records_when_handled_then_should_count_only_those() {
        let mut handler = RegionalHandler::new(10);
        let at = Delivered {
            partition: 0,
            offset: 1,
        };
        handler.handle(&fixtures::reefer_fault(), at);
        handler.handle(&fixtures::enters_unsafe(), at);
        assert_eq!(
            (handler.readings, handler.changes, handler.trucks.len()),
            (1, 1, 1)
        );
    }
}
