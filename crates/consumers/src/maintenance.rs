use crate::handler::{BoundedTable, Delivered, Handler};
use frostline_shared::domain::TruckId;
use frostline_shared::event::FleetEvent;

/// Maintenance keeps the open refrigeration faults by truck.
pub struct MaintenanceHandler {
    faults: BoundedTable<TruckId, Option<u16>>,
    handled: u64,
}

impl MaintenanceHandler {
    pub fn new(bound: usize) -> Self {
        Self {
            faults: BoundedTable::new(bound),
            handled: 0,
        }
    }
}

impl Handler for MaintenanceHandler {
    fn handle(&mut self, event: &FleetEvent, _delivered: Delivered) {
        let Some(truck) = &event.truck_id else {
            return;
        };
        self.handled += 1;
        let code = event
            .telemetry
            .as_ref()
            .and_then(|telemetry| telemetry.fault_code);
        self.faults.insert(truck.clone(), code);
    }

    fn narrate(&self, event: &FleetEvent, delivered: Delivered) -> Option<String> {
        let truck = event.truck_id.as_ref()?;
        let (unit, severity) = event.diagnosis();
        let code = event
            .telemetry
            .as_ref()
            .and_then(|telemetry| telemetry.fault_code)
            .map_or_else(String::new, |code| format!(" E{code}"));
        Some(format!(
            "Fault{code} on the {unit} of {truck}, severity {severity}. Maintenance received partition {} offset {} and the server never decoded it.",
            delivered.partition, delivered.offset
        ))
    }

    fn status(&self) -> String {
        format!(
            "{} faults handled, {} trucks with an open fault",
            self.handled,
            self.faults.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use frostline_shared::fixtures;

    #[test]
    fn given_a_reefer_fault_when_handled_and_narrated_then_should_track_and_describe_it() {
        let mut handler = MaintenanceHandler::new(10);
        let at = Delivered {
            partition: 1,
            offset: 233,
        };
        handler.handle(&fixtures::reefer_fault(), at);
        assert_eq!(handler.faults.len(), 1);
        assert_eq!(
            handler
                .narrate(&fixtures::reefer_fault(), at)
                .expect("a line"),
            "Fault E17 on the reefer of FR-042, severity error. Maintenance received partition 1 offset 233 and the server never decoded it."
        );
    }
}
