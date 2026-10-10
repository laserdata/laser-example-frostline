mod protobuf;
mod schemas;

pub use schemas::{AVRO_SCHEMA, SchemaSet, drop_schemas, load_schemas, register_schemas};

use crate::event::FleetEvent;
use crate::names::Header;
use laser_sdk::filters::{ConsumerFilter, FilterExpr, FilterHeader, HeaderScalar};
use laser_sdk::iggy::prelude::{HeaderKey, HeaderKind, HeaderValue, IggyError};
use laser_sdk::prelude::ContentType;
use laser_sdk::wire::headers::{CONTENT_TYPE, SCHEMA_ID};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::str::FromStr;
use strum::{Display, EnumIter, EnumString};
use thiserror::Error;

pub type Headers = BTreeMap<HeaderKey, HeaderValue>;

/// The payload format of one run. Every record of the run, checkpoints included, uses it.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Deserialize,
    Display,
    EnumIter,
    EnumString,
    Eq,
    Hash,
    PartialEq,
    Serialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Codec {
    #[default]
    Json,
    Cbor,
    Avro,
    Protobuf,
}

#[derive(Debug, Error)]
pub enum CodecError {
    #[error("JSON codec failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("CBOR codec failed: {0}")]
    Cbor(String),
    #[error("Avro codec failed: {0}")]
    Avro(String),
    #[error("Protobuf codec failed: {0}")]
    Protobuf(String),
    #[error("the {0} codec needs its registered writer schema")]
    MissingSchema(Codec),
    #[error("record headers are invalid: {0}")]
    Header(#[from] IggyError),
}

impl Codec {
    pub fn content_type(&self) -> ContentType {
        match self {
            Codec::Json => ContentType::Json,
            Codec::Cbor => ContentType::Cbor,
            Codec::Avro => ContentType::Avro,
            Codec::Protobuf => ContentType::Protobuf,
        }
    }

    pub fn needs_schema(&self) -> bool {
        matches!(self, Codec::Avro | Codec::Protobuf)
    }

    pub fn encode(&self, event: &FleetEvent, schemas: &SchemaSet) -> Result<Vec<u8>, CodecError> {
        match self {
            Codec::Json => Ok(serde_json::to_vec(event)?),
            Codec::Cbor => {
                let mut bytes = Vec::new();
                ciborium::into_writer(event, &mut bytes)
                    .map_err(|error| CodecError::Cbor(error.to_string()))?;
                Ok(bytes)
            }
            Codec::Avro => schemas
                .avro()?
                .encode_avro(event)
                .map_err(|error| CodecError::Avro(error.to_string())),
            Codec::Protobuf => protobuf::encode(event),
        }
    }

    pub fn decode(&self, bytes: &[u8], schemas: &SchemaSet) -> Result<FleetEvent, CodecError> {
        match self {
            Codec::Json => Ok(serde_json::from_slice(bytes)?),
            Codec::Cbor => {
                ciborium::from_reader(bytes).map_err(|error| CodecError::Cbor(error.to_string()))
            }
            Codec::Avro => {
                let value = schemas
                    .avro()?
                    .decode(bytes)
                    .map_err(|error| CodecError::Avro(error.to_string()))?;
                serde_json::from_value(value).map_err(|error| CodecError::Avro(error.to_string()))
            }
            Codec::Protobuf => protobuf::decode(bytes),
        }
    }

    /// The consumer filter for `expr` under this codec. Avro and Protobuf name their writer schemas.
    pub fn wrap(&self, expr: FilterExpr, schema_refs: &[u32]) -> ConsumerFilter {
        match self {
            Codec::Json => ConsumerFilter::json(expr),
            Codec::Cbor => ConsumerFilter::cbor(expr),
            Codec::Avro => ConsumerFilter::avro(expr, schema_refs.iter().copied()),
            Codec::Protobuf => ConsumerFilter::protobuf(expr, schema_refs.iter().copied()),
        }
    }

    /// The typed headers of one record: the Frostline domain headers, the content type, and the schema id.
    pub fn headers(&self, event: &FleetEvent, schemas: &SchemaSet) -> Result<Headers, CodecError> {
        let (unit, severity) = event.diagnosis();
        let mut headers = Headers::from([
            (Header::Frame.key(), HeaderValue::from(event.frame() as u8)),
            (
                Header::Unit.key(),
                HeaderValue::try_from(unit.to_string().as_str())?,
            ),
            (Header::Severity.key(), HeaderValue::from(severity as u8)),
            (
                HeaderKey::from_str(CONTENT_TYPE)?,
                HeaderValue::from(self.content_type().code()),
            ),
        ]);
        if let Some(schema_id) = schemas.id_for(*self) {
            headers.insert(
                HeaderKey::from_str(SCHEMA_ID)?,
                HeaderValue::from(schema_id),
            );
        }
        Ok(headers)
    }
}

/// The typed headers of a record in the shape a filter sample test takes.
pub fn filter_headers(headers: &Headers) -> Result<Vec<FilterHeader>, CodecError> {
    headers
        .iter()
        .map(|(key, value)| {
            let scalar = match value.kind() {
                HeaderKind::Uint8 => HeaderScalar::Uint(u64::from(u8::try_from(value)?)),
                HeaderKind::Uint32 => HeaderScalar::Uint(u64::from(u32::try_from(value)?)),
                HeaderKind::String => HeaderScalar::String(value.as_str()?.to_owned()),
                _ => HeaderScalar::Raw(Vec::<u8>::try_from(value)?),
            };
            Ok(FilterHeader {
                key: key.as_str()?.to_owned(),
                value: scalar,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
