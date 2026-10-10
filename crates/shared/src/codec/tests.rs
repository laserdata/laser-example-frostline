use super::*;
use crate::domain::Cargo;
use crate::fixtures;
use laser_sdk::filters::Verdict;
use laser_sdk::query::CmpOp;
use laser_sdk::wire::filter::eval::{CompiledFilter, DecodeLimits, FilterRecord};
use prost::Message;
use std::path::Path;
use strum::IntoEnumIterator;

const REGEN_DESCRIPTOR: &str = "FROSTLINE_REGEN_DESCRIPTOR";
const AVRO_ID: u32 = 7;
const PROTOBUF_ID: u32 = 8;

#[test]
fn given_every_codec_when_scenario_events_round_trip_then_should_decode_to_the_same_event() {
    for codec in Codec::iter() {
        let schemas = schemas(codec);
        for event in scenario() {
            let bytes = codec.encode(&event, &schemas).expect("event encodes");
            let decoded = codec.decode(&bytes, &schemas).expect("event decodes");
            assert_eq!(decoded, event, "{codec}");
        }
    }
}

#[test]
fn given_a_schema_codec_without_its_schema_when_encoding_then_should_fail() {
    let error = Codec::Avro
        .encode(&fixtures::enters_unsafe(), &SchemaSet::default())
        .expect_err("Avro needs a schema");
    assert!(matches!(error, CodecError::MissingSchema(Codec::Avro)));
}

#[test]
fn given_a_business_record_when_headers_are_built_then_should_carry_typed_values() {
    let headers = Codec::Avro
        .headers(&fixtures::reefer_fault(), &schemas(Codec::Avro))
        .expect("headers build");
    let value = |name: &str| {
        headers
            .get(&HeaderKey::from_str(name).expect("header key"))
            .expect("header present")
    };
    assert_eq!(u8::try_from(value("frostline.frame")).expect("uint8"), 0);
    assert_eq!(
        String::try_from(value("frostline.unit")).expect("string"),
        "reefer"
    );
    assert_eq!(u8::try_from(value("frostline.severity")).expect("uint8"), 2);
    assert_eq!(u8::try_from(value(CONTENT_TYPE)).expect("uint8"), 5);
    assert_eq!(u32::try_from(value(SCHEMA_ID)).expect("uint32"), AVRO_ID);
    assert_eq!(headers.len(), 5);
}

#[test]
fn given_a_json_checkpoint_when_headers_are_built_then_should_mark_the_frame_without_a_schema() {
    let headers = Codec::Json
        .headers(&fixtures::checkpoint(1), &SchemaSet::default())
        .expect("headers build");
    let frame = headers.get(&Header::Frame.key()).expect("frame header");
    assert_eq!(u8::try_from(frame).expect("uint8"), 1);
    assert!(!headers.contains_key(&HeaderKey::from_str(SCHEMA_ID).expect("key")));
}

#[test]
fn given_a_regional_predicate_when_evaluated_on_cbor_bytes_then_should_select_the_record() {
    let filter = Codec::Cbor.wrap(
        FilterExpr::all([
            FilterExpr::pred("region", CmpOp::Eq, "north"),
            FilterExpr::pred("change.after.cargo", CmpOp::Eq, Cargo::Pharma.to_string()),
        ]),
        &[],
    );
    let compiled = CompiledFilter::compile(&filter).expect("filter compiles");
    let bytes = Codec::Cbor
        .encode(&fixtures::enters_unsafe(), &SchemaSet::default())
        .expect("event encodes");
    let verdict = compiled.evaluate(
        &FilterRecord {
            payload: &bytes,
            headers: &[],
        },
        &DecodeLimits::default(),
    );
    assert_eq!(verdict, Verdict::Selected);
}

#[test]
fn given_schema_codecs_when_wrapped_then_should_name_their_writer_schema() {
    let expr = FilterExpr::pred("kind", CmpOp::Eq, "change");
    assert_eq!(
        Codec::Avro.wrap(expr.clone(), &[AVRO_ID]).schema_refs,
        [AVRO_ID]
    );
    assert!(Codec::Json.wrap(expr, &[AVRO_ID]).schema_refs.is_empty());
}

#[test]
fn given_the_avro_schema_when_parsed_then_should_be_valid() {
    apache_avro::Schema::parse_str(AVRO_SCHEMA).expect("the checked-in Avro schema parses");
}

#[test]
fn given_the_proto_source_when_compiled_then_should_equal_the_checked_in_descriptor() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let compiled = protox::compile(["fleet-event.proto"], [&root])
        .expect("the proto source compiles")
        .encode_to_vec();
    if std::env::var_os(REGEN_DESCRIPTOR).is_some() {
        std::fs::write(root.join("fleet-event.desc"), &compiled).expect("descriptor writes");
    }
    assert_eq!(
        compiled,
        protobuf::DESCRIPTOR,
        "run with {REGEN_DESCRIPTOR}=1 to refresh"
    );
}

fn scenario() -> Vec<FleetEvent> {
    vec![
        fixtures::enters_unsafe(),
        fixtures::retired(Cargo::Frozen),
        fixtures::reefer_fault(),
        fixtures::checkpoint(2),
    ]
}

fn schemas(codec: Codec) -> SchemaSet {
    let id = if codec == Codec::Avro {
        AVRO_ID
    } else {
        PROTOBUF_ID
    };
    SchemaSet::local(codec, id).expect("local schema compiles")
}
