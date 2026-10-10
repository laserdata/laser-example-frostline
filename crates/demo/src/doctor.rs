use crate::error::DemoError;
use frostline_shared::codec::Codec;
use frostline_shared::output::{capability, fact, phase};
use frostline_shared::{LaserFactory, Settings};
use laser_sdk::filters::FilterCodec;
use laser_sdk::prelude::Capabilities;

const CLOUD_HINT: &str = "use Laser Stack with the LaserData Iggy fork, or LaserData Cloud";

/// What the connected server offers for this demo.
#[derive(Clone, Debug, PartialEq)]
pub struct Doctor {
    pub target: String,
    pub native: bool,
    pub catalog: bool,
    pub codecs: Option<Vec<FilterCodec>>,
}

impl Doctor {
    pub fn inspect(target: String, capabilities: &Capabilities) -> Self {
        Self {
            target,
            native: capabilities.filters.native,
            catalog: capabilities.filters.catalog,
            codecs: capabilities
                .filters
                .evaluation
                .as_ref()
                .map(|announce| announce.codecs.clone()),
        }
    }

    /// Fail with one sentence per missing capability the settings need.
    pub fn require(&self, settings: &Settings) -> Result<(), DemoError> {
        let mut missing = Vec::new();
        if !self.native {
            missing.push("It does not serve consumer filters.".to_owned());
        }
        if !self.catalog {
            missing.push(
                "Group-owned filters need a ready plane, including the inline profile.".to_owned(),
            );
        }
        if settings.codec.needs_schema() && !self.catalog {
            missing.push(
                "The writer schema for this codec needs a ready plane, including inline runs."
                    .to_owned(),
            );
        }
        let wanted = filter_codec(settings.codec);
        if self.native
            && self
                .codecs
                .as_ref()
                .is_some_and(|codecs| !codecs.contains(&wanted))
        {
            missing.push(format!("It does not evaluate {} payloads.", settings.codec));
        }
        if missing.is_empty() {
            return Ok(());
        }
        Err(DemoError::Preflight(missing.join(" ")))
    }

    pub fn print(&self) {
        phase("doctor");
        fact("target", &self.target);
        capability("consumer filters", self.native, CLOUD_HINT);
        capability(
            "saved filters and group bindings",
            self.catalog,
            "needs a ready laser-plane",
        );
        match &self.codecs {
            Some(codecs) => {
                let names: Vec<String> = codecs.iter().map(ToString::to_string).collect();
                fact("codecs", names.join(", "));
            }
            None => fact("codecs", "the server did not announce its codecs"),
        }
    }
}

pub async fn run(factory: &LaserFactory) -> Result<Doctor, DemoError> {
    let laser = factory.connect("frostline-doctor").await?;
    let capabilities = laser.refresh_capabilities().await;
    let doctor = Doctor::inspect(factory.target().to_string(), &capabilities);
    laser.close().await?;
    Ok(doctor)
}

pub fn filter_codec(codec: Codec) -> FilterCodec {
    match codec {
        Codec::Json => FilterCodec::Json,
        Codec::Cbor => FilterCodec::Cbor,
        Codec::Avro => FilterCodec::Avro,
        Codec::Protobuf => FilterCodec::Protobuf,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use frostline_shared::config::{Catalog, Mode};

    #[test]
    fn given_a_server_without_filters_when_required_then_should_say_so() {
        let doctor = Doctor {
            target: "t".to_owned(),
            native: false,
            catalog: false,
            codecs: None,
        };
        let error = doctor
            .require(&Settings::defaults(Mode::Finite))
            .expect_err("filters are missing");
        assert_eq!(
            error.to_string(),
            "The server cannot run this demo. It does not serve consumer filters. Group-owned filters need a ready plane, including the inline profile."
        );
    }

    #[test]
    fn given_inline_mode_without_a_catalog_when_required_then_should_reject() {
        let doctor = Doctor {
            target: "t".to_owned(),
            native: true,
            catalog: false,
            codecs: Some(vec![FilterCodec::Json]),
        };
        let settings = Settings {
            catalog: Catalog::Inline,
            ..Settings::defaults(Mode::Finite)
        };
        assert_eq!(
            doctor
                .require(&settings)
                .expect_err("group policies need the catalog")
                .to_string(),
            "The server cannot run this demo. Group-owned filters need a ready plane, including the inline profile."
        );
    }

    #[test]
    fn given_an_unannounced_codec_when_required_then_should_name_it() {
        let doctor = Doctor {
            target: "t".to_owned(),
            native: true,
            catalog: true,
            codecs: Some(vec![FilterCodec::Json]),
        };
        let settings = Settings {
            codec: Codec::Avro,
            ..Settings::defaults(Mode::Finite)
        };
        assert_eq!(
            doctor
                .require(&settings)
                .expect_err("Avro is missing")
                .to_string(),
            "The server cannot run this demo. It does not evaluate avro payloads."
        );
    }
}
