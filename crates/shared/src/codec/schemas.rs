use super::protobuf::{DESCRIPTOR, MESSAGE_TYPE};
use super::{Codec, CodecError};
use laser_sdk::prelude::{Laser, LaserError};
use laser_sdk::query::{SchemaDef, SchemaSource};
use laser_sdk::schema_codecs::CompiledSchema;
use std::time::Duration;
use tokio::time::{sleep, timeout};

pub const AVRO_SCHEMA: &str = include_str!("../../../../schemas/fleet-event.avsc");
const VISIBILITY_TIMEOUT: Duration = Duration::from_secs(15);
const VISIBILITY_POLL: Duration = Duration::from_millis(50);

/// The writer schemas one run registered, compiled where the client decodes with them.
#[derive(Clone, Debug, Default)]
pub struct SchemaSet {
    avro: Option<(u32, CompiledSchema)>,
    protobuf: Option<u32>,
}

impl SchemaSet {
    pub fn avro(&self) -> Result<&CompiledSchema, CodecError> {
        self.avro
            .as_ref()
            .map(|(_, compiled)| compiled)
            .ok_or(CodecError::MissingSchema(Codec::Avro))
    }

    pub fn id_for(&self, codec: Codec) -> Option<u32> {
        match codec {
            Codec::Avro => self.avro.as_ref().map(|(id, _)| *id),
            Codec::Protobuf => self.protobuf,
            Codec::Json | Codec::Cbor => None,
        }
    }

    pub fn ids(&self) -> Vec<u32> {
        self.id_for(Codec::Avro)
            .into_iter()
            .chain(self.protobuf)
            .collect()
    }

    /// A schema set compiled from a source without the registry, for tests and local checks.
    pub fn local(codec: Codec, id: u32) -> Result<Self, LaserError> {
        let definition = definition(codec, id);
        Ok(match (codec, definition) {
            (Codec::Avro, Some(definition)) => Self {
                avro: Some((id, CompiledSchema::compile(&definition)?)),
                protobuf: None,
            },
            (Codec::Protobuf, Some(_)) => Self {
                avro: None,
                protobuf: Some(id),
            },
            _ => Self::default(),
        })
    }
}

/// Register the writer schema `codec` needs, then wait until the registry serves it.
pub async fn register_schemas(laser: &Laser, codec: Codec) -> Result<SchemaSet, LaserError> {
    let Some(source) = source(codec) else {
        return Ok(SchemaSet::default());
    };
    // A plane that is still replaying its control log refuses schema calls as
    // retryable, so both steps retry within the visibility budget.
    let id = timeout(VISIBILITY_TIMEOUT, async {
        let id = loop {
            let registered = laser
                .schemas()
                .register(source.clone())
                .name(format!("frostline-fleet-event-{codec}"))
                .send()
                .await;
            match registered {
                Err(error) if error.is_retryable() => sleep(VISIBILITY_POLL).await,
                other => break other?,
            }
        };
        loop {
            match laser.schemas().get(id).await {
                Ok(Some(_)) => return Ok::<u32, LaserError>(id),
                Ok(None) => {}
                Err(error) if error.is_retryable() => {}
                Err(error) => return Err(error),
            }
            sleep(VISIBILITY_POLL).await;
        }
    })
    .await
    .map_err(|_| LaserError::Timeout("the registered writer schema to become visible"))??;
    SchemaSet::local(codec, id)
}

/// Rebuild the schema set of an existing run from the ids its run file recorded.
pub async fn load_schemas(
    laser: &Laser,
    codec: Codec,
    ids: &[u32],
) -> Result<SchemaSet, LaserError> {
    let Some(&id) = ids.first() else {
        return Ok(SchemaSet::default());
    };
    let info = laser
        .schemas()
        .get(id)
        .await?
        .ok_or_else(|| LaserError::Invalid(format!("writer schema {id} is not registered")))?;
    Ok(match codec {
        Codec::Avro => SchemaSet {
            avro: Some((id, CompiledSchema::compile(&info.schema)?)),
            protobuf: None,
        },
        Codec::Protobuf => SchemaSet {
            avro: None,
            protobuf: Some(id),
        },
        Codec::Json | Codec::Cbor => SchemaSet::default(),
    })
}

pub async fn drop_schemas(laser: &Laser, ids: &[u32]) -> Result<(), LaserError> {
    for &id in ids {
        laser.schemas().drop(id).await?;
    }
    Ok(())
}

fn source(codec: Codec) -> Option<SchemaSource> {
    match codec {
        Codec::Avro => Some(SchemaSource::Avro {
            schema: AVRO_SCHEMA.to_owned(),
        }),
        Codec::Protobuf => Some(SchemaSource::Protobuf {
            descriptor_set: DESCRIPTOR.to_vec(),
            message_type: MESSAGE_TYPE.to_owned(),
        }),
        Codec::Json | Codec::Cbor => None,
    }
}

fn definition(codec: Codec, id: u32) -> Option<SchemaDef> {
    source(codec).map(|source| SchemaDef {
        id,
        source,
        name: None,
        version: None,
    })
}
