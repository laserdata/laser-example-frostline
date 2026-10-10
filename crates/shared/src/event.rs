use crate::domain::{
    BatteryPercent, Cargo, ContractVersion, DeciCelsius, Field, LogicalTime, Operation, Region,
    Sequence, TemperatureBand, TruckId, TruckState, UnitState, WindowId,
};
use crate::names::{Frame, RunId, Severity, Unit};
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};
use thiserror::Error;

const LOW_BATTERY_PERCENT: u8 = 15;

/// One record on the `changes` topic: a CDC change, a telemetry reading, or a window checkpoint.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct FleetEvent {
    #[serde(rename = "v")]
    pub version: ContractVersion,
    pub run_id: RunId,
    pub event_id: u64,
    pub window_id: WindowId,
    pub sequence: Sequence,
    pub logical_time_micros: LogicalTime,
    pub truck_id: Option<TruckId>,
    pub region: Option<Region>,
    pub cargo: Option<Cargo>,
    pub kind: EventKind,
    pub change: Option<Change>,
    pub telemetry: Option<Telemetry>,
    pub checkpoint: Option<Checkpoint>,
}

/// The envelope fields every event carries, filled by the producer.
#[derive(Clone, Debug, PartialEq)]
pub struct EventHeader {
    pub run_id: RunId,
    pub event_id: u64,
    pub window_id: WindowId,
    pub sequence: Sequence,
    pub logical_time: LogicalTime,
}

#[derive(Clone, Copy, Debug, Deserialize, Display, EnumString, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum EventKind {
    Change,
    Telemetry,
    Checkpoint,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Change {
    pub op: Operation,
    pub before: Option<TruckState>,
    pub after: Option<TruckState>,
    pub changed: Vec<Field>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Telemetry {
    pub temperature_deci_c: DeciCelsius,
    pub battery_pct: BatteryPercent,
    pub unit_state: UnitState,
    pub fault_code: Option<u16>,
    pub latitude_e6: i32,
    pub longitude_e6: i32,
    pub samples: Vec<i16>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Checkpoint {
    pub window_id: WindowId,
    pub partition_id: u32,
    /// Set on the checkpoint that closes the run.
    pub last: bool,
}

#[derive(Debug, Error, PartialEq)]
pub enum EventError {
    #[error("a {kind} event needs exactly its own body")]
    BodyMismatch { kind: EventKind },
    #[error("an {op} change carries no changed fields")]
    ChangedOnNonUpdate { op: Operation },
    #[error("an update must change at least one field")]
    EmptyChanged,
    #[error("changed fields {got:?} do not match the before and after diff {expected:?}")]
    ChangedDisagrees {
        expected: Vec<Field>,
        got: Vec<Field>,
    },
    #[error("an {op} change has the wrong before and after shape")]
    ChangeShape { op: Operation },
    #[error("a checkpoint names no truck")]
    CheckpointWithTruck,
}

impl FleetEvent {
    pub fn change(header: EventHeader, truck: &TruckId, change: Change) -> Self {
        let state = change.after.as_ref().or(change.before.as_ref());
        Self {
            region: state.map(|state| state.region),
            cargo: state.map(|state| state.cargo),
            change: Some(change),
            ..Self::bare(header, Some(truck.clone()), EventKind::Change)
        }
    }

    pub fn telemetry(
        header: EventHeader,
        truck: &TruckId,
        state: &TruckState,
        telemetry: Telemetry,
    ) -> Self {
        Self {
            region: Some(state.region),
            cargo: Some(state.cargo),
            telemetry: Some(telemetry),
            ..Self::bare(header, Some(truck.clone()), EventKind::Telemetry)
        }
    }

    pub fn checkpoint(header: EventHeader, partition_id: u32, last: bool) -> Self {
        let window_id = header.window_id;
        Self {
            checkpoint: Some(Checkpoint {
                window_id,
                partition_id,
                last,
            }),
            ..Self::bare(header, None, EventKind::Checkpoint)
        }
    }

    pub fn validate(&self) -> Result<(), EventError> {
        let bodies = (
            self.change.is_some(),
            self.telemetry.is_some(),
            self.checkpoint.is_some(),
        );
        let expected = match self.kind {
            EventKind::Change => (true, false, false),
            EventKind::Telemetry => (false, true, false),
            EventKind::Checkpoint => (false, false, true),
        };
        if bodies != expected {
            return Err(EventError::BodyMismatch { kind: self.kind });
        }
        if self.kind == EventKind::Checkpoint && self.truck_id.is_some() {
            return Err(EventError::CheckpointWithTruck);
        }
        self.change.as_ref().map_or(Ok(()), Change::validate)
    }

    pub fn frame(&self) -> Frame {
        match self.kind {
            EventKind::Checkpoint => Frame::Checkpoint,
            EventKind::Change | EventKind::Telemetry => Frame::Business,
        }
    }

    /// The subsystem and severity the producer stamps as typed headers.
    pub fn diagnosis(&self) -> (Unit, Severity) {
        if let Some(telemetry) = &self.telemetry {
            if telemetry.fault_code.is_some() {
                return (Unit::Reefer, Severity::Error);
            }
            if telemetry.battery_pct.get() < LOW_BATTERY_PERCENT {
                return (Unit::Battery, Severity::Warning);
            }
        }
        if let Some(change) = &self.change
            && change.enters_unsafe()
        {
            return (Unit::Reefer, Severity::Critical);
        }
        (Unit::Engine, Severity::Info)
    }

    fn bare(header: EventHeader, truck_id: Option<TruckId>, kind: EventKind) -> Self {
        Self {
            version: ContractVersion::CURRENT,
            run_id: header.run_id,
            event_id: header.event_id,
            window_id: header.window_id,
            sequence: header.sequence,
            logical_time_micros: header.logical_time,
            truck_id,
            region: None,
            cargo: None,
            kind,
            change: None,
            telemetry: None,
            checkpoint: None,
        }
    }
}

impl Change {
    pub fn update(before: TruckState, after: TruckState) -> Self {
        let changed = before.diff(&after);
        Self {
            op: Operation::Update,
            before: Some(before),
            after: Some(after),
            changed,
        }
    }

    pub fn insert(after: TruckState) -> Self {
        Self {
            op: Operation::Insert,
            before: None,
            after: Some(after),
            changed: Vec::new(),
        }
    }

    pub fn delete(before: TruckState) -> Self {
        Self {
            op: Operation::Delete,
            before: Some(before),
            after: None,
            changed: Vec::new(),
        }
    }

    pub fn enters_unsafe(&self) -> bool {
        match (&self.before, &self.after) {
            (Some(before), Some(after)) => {
                before.temperature_band != TemperatureBand::Unsafe
                    && after.temperature_band == TemperatureBand::Unsafe
            }
            _ => false,
        }
    }

    fn validate(&self) -> Result<(), EventError> {
        let shape = match self.op {
            Operation::Insert => self.before.is_none() && self.after.is_some(),
            Operation::Update => self.before.is_some() && self.after.is_some(),
            Operation::Delete => self.before.is_some() && self.after.is_none(),
        };
        if !shape {
            return Err(EventError::ChangeShape { op: self.op });
        }
        let (Some(before), Some(after)) = (&self.before, &self.after) else {
            return match self.changed.is_empty() {
                true => Ok(()),
                false => Err(EventError::ChangedOnNonUpdate { op: self.op }),
            };
        };
        let expected = before.diff(after);
        if expected.is_empty() {
            return Err(EventError::EmptyChanged);
        }
        if expected != self.changed {
            return Err(EventError::ChangedDisagrees {
                expected,
                got: self.changed.clone(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
