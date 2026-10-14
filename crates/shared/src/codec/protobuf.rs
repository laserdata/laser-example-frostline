use super::CodecError;
use crate::domain::{
    BatteryPercent, ContractVersion, DeciCelsius, LogicalTime, Sequence, TruckId, TruckState,
    WindowId,
};
use crate::event::{Change, Checkpoint, FleetEvent, Telemetry};
use prost::Message;
use std::str::FromStr;

pub const MESSAGE_TYPE: &str = "frostline.FleetEvent";
pub const DESCRIPTOR: &[u8] = include_bytes!("../../../../schemas/fleet-event.desc");

#[derive(Clone, PartialEq, Message)]
struct FleetEventProto {
    #[prost(uint32, tag = "1")]
    v: u32,
    #[prost(string, tag = "2")]
    run_id: String,
    #[prost(uint64, tag = "3")]
    event_id: u64,
    #[prost(uint64, tag = "4")]
    window_id: u64,
    #[prost(uint64, tag = "5")]
    sequence: u64,
    #[prost(uint64, tag = "6")]
    logical_time_micros: u64,
    #[prost(string, optional, tag = "7")]
    truck_id: Option<String>,
    #[prost(string, optional, tag = "8")]
    region: Option<String>,
    #[prost(string, optional, tag = "9")]
    cargo: Option<String>,
    #[prost(string, tag = "10")]
    kind: String,
    #[prost(message, optional, tag = "11")]
    change: Option<ChangeProto>,
    #[prost(message, optional, tag = "12")]
    telemetry: Option<TelemetryProto>,
    #[prost(message, optional, tag = "13")]
    checkpoint: Option<CheckpointProto>,
}

#[derive(Clone, PartialEq, Message)]
struct ChangeProto {
    #[prost(string, tag = "1")]
    op: String,
    #[prost(message, optional, tag = "2")]
    before: Option<TruckStateProto>,
    #[prost(message, optional, tag = "3")]
    after: Option<TruckStateProto>,
    #[prost(string, repeated, tag = "4")]
    changed: Vec<String>,
}

#[derive(Clone, PartialEq, Message)]
struct TruckStateProto {
    #[prost(string, tag = "1")]
    temperature_band: String,
    #[prost(sint32, tag = "2")]
    temperature_deci_c: i32,
    #[prost(uint32, tag = "3")]
    battery_pct: u32,
    #[prost(string, tag = "4")]
    unit_state: String,
    #[prost(string, tag = "5")]
    trip_status: String,
    #[prost(string, tag = "6")]
    region: String,
    #[prost(string, tag = "7")]
    cargo: String,
    #[prost(string, tag = "8")]
    declared_weight_tonnes: String,
}

#[derive(Clone, PartialEq, Message)]
struct TelemetryProto {
    #[prost(sint32, tag = "1")]
    temperature_deci_c: i32,
    #[prost(uint32, tag = "2")]
    battery_pct: u32,
    #[prost(string, tag = "3")]
    unit_state: String,
    #[prost(uint32, optional, tag = "4")]
    fault_code: Option<u32>,
    #[prost(sint32, tag = "5")]
    latitude_e6: i32,
    #[prost(sint32, tag = "6")]
    longitude_e6: i32,
    #[prost(sint32, repeated, tag = "7")]
    samples: Vec<i32>,
}

#[derive(Clone, PartialEq, Message)]
struct CheckpointProto {
    #[prost(uint64, tag = "1")]
    window_id: u64,
    #[prost(uint32, tag = "2")]
    partition_id: u32,
    #[prost(bool, tag = "3")]
    last: bool,
}

pub fn encode(event: &FleetEvent) -> Result<Vec<u8>, CodecError> {
    Ok(to_proto(event).encode_to_vec())
}

pub fn decode(bytes: &[u8]) -> Result<FleetEvent, CodecError> {
    let proto = FleetEventProto::decode(bytes).map_err(invalid)?;
    from_proto(proto)
}

fn to_proto(event: &FleetEvent) -> FleetEventProto {
    FleetEventProto {
        v: u32::from(event.version.0),
        run_id: event.run_id.to_string(),
        event_id: event.event_id,
        window_id: event.window_id.0,
        sequence: event.sequence.0,
        logical_time_micros: event.logical_time_micros.0,
        truck_id: event.truck_id.as_ref().map(ToString::to_string),
        region: event.region.map(|region| region.to_string()),
        cargo: event.cargo.map(|cargo| cargo.to_string()),
        kind: event.kind.to_string(),
        change: event.change.as_ref().map(change_to_proto),
        telemetry: event.telemetry.as_ref().map(|telemetry| TelemetryProto {
            temperature_deci_c: telemetry.temperature_deci_c.0,
            battery_pct: u32::from(telemetry.battery_pct.get()),
            unit_state: telemetry.unit_state.to_string(),
            fault_code: telemetry.fault_code.map(u32::from),
            latitude_e6: telemetry.latitude_e6,
            longitude_e6: telemetry.longitude_e6,
            samples: telemetry.samples.iter().copied().map(i32::from).collect(),
        }),
        checkpoint: event.checkpoint.as_ref().map(|checkpoint| CheckpointProto {
            window_id: checkpoint.window_id.0,
            partition_id: checkpoint.partition_id,
            last: checkpoint.last,
        }),
    }
}

fn change_to_proto(change: &Change) -> ChangeProto {
    ChangeProto {
        op: change.op.to_string(),
        before: change.before.as_ref().map(state_to_proto),
        after: change.after.as_ref().map(state_to_proto),
        changed: change.changed.iter().map(ToString::to_string).collect(),
    }
}

fn state_to_proto(state: &TruckState) -> TruckStateProto {
    TruckStateProto {
        temperature_band: state.temperature_band.to_string(),
        temperature_deci_c: state.temperature_deci_c.0,
        battery_pct: u32::from(state.battery_pct.get()),
        unit_state: state.unit_state.to_string(),
        trip_status: state.trip_status.to_string(),
        region: state.region.to_string(),
        cargo: state.cargo.to_string(),
        declared_weight_tonnes: state.declared_weight_tonnes.clone(),
    }
}

fn from_proto(proto: FleetEventProto) -> Result<FleetEvent, CodecError> {
    Ok(FleetEvent {
        version: ContractVersion(u16::try_from(proto.v).map_err(invalid)?),
        run_id: parse(&proto.run_id)?,
        event_id: proto.event_id,
        window_id: WindowId(proto.window_id),
        sequence: Sequence(proto.sequence),
        logical_time_micros: LogicalTime(proto.logical_time_micros),
        truck_id: proto
            .truck_id
            .as_deref()
            .map(parse::<TruckId>)
            .transpose()?,
        region: proto.region.as_deref().map(parse).transpose()?,
        cargo: proto.cargo.as_deref().map(parse).transpose()?,
        kind: parse(&proto.kind)?,
        change: proto.change.map(change_from_proto).transpose()?,
        telemetry: proto
            .telemetry
            .map(|telemetry| {
                Ok::<_, CodecError>(Telemetry {
                    temperature_deci_c: DeciCelsius(telemetry.temperature_deci_c),
                    battery_pct: battery(telemetry.battery_pct)?,
                    unit_state: parse(&telemetry.unit_state)?,
                    fault_code: telemetry
                        .fault_code
                        .map(|code| u16::try_from(code).map_err(invalid))
                        .transpose()?,
                    latitude_e6: telemetry.latitude_e6,
                    longitude_e6: telemetry.longitude_e6,
                    samples: telemetry
                        .samples
                        .into_iter()
                        .map(|sample| i16::try_from(sample).map_err(invalid))
                        .collect::<Result<_, _>>()?,
                })
            })
            .transpose()?,
        checkpoint: proto.checkpoint.map(|checkpoint| Checkpoint {
            window_id: WindowId(checkpoint.window_id),
            partition_id: checkpoint.partition_id,
            last: checkpoint.last,
        }),
    })
}

fn change_from_proto(change: ChangeProto) -> Result<Change, CodecError> {
    Ok(Change {
        op: parse(&change.op)?,
        before: change.before.map(state_from_proto).transpose()?,
        after: change.after.map(state_from_proto).transpose()?,
        changed: change
            .changed
            .iter()
            .map(String::as_str)
            .map(parse)
            .collect::<Result<_, _>>()?,
    })
}

fn state_from_proto(state: TruckStateProto) -> Result<TruckState, CodecError> {
    Ok(TruckState {
        temperature_band: parse(&state.temperature_band)?,
        temperature_deci_c: DeciCelsius(state.temperature_deci_c),
        battery_pct: battery(state.battery_pct)?,
        unit_state: parse(&state.unit_state)?,
        trip_status: parse(&state.trip_status)?,
        region: parse(&state.region)?,
        cargo: parse(&state.cargo)?,
        declared_weight_tonnes: state.declared_weight_tonnes,
    })
}

fn battery(value: u32) -> Result<BatteryPercent, CodecError> {
    let value = u8::try_from(value).map_err(invalid)?;
    BatteryPercent::new(value).map_err(invalid)
}

fn parse<T: FromStr>(text: &str) -> Result<T, CodecError>
where
    T::Err: std::fmt::Display,
{
    text.parse::<T>().map_err(invalid)
}

fn invalid(error: impl std::fmt::Display) -> CodecError {
    CodecError::Protobuf(error.to_string())
}
