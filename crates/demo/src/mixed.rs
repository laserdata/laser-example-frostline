use crate::error::DemoError;
use crate::provision::Provisioned;
use frostline_shared::codec::{Codec, Headers, SchemaSet};
use frostline_shared::fixtures;
use frostline_shared::names::Role;
use frostline_shared::output::act;
use frostline_shared::policy::RolePolicy;
use laser_sdk::filters::{
    ConsumerFilter, FaultPolicy, FaultReason, FilterExpr, FilteredStart, RecordPolicy, TextMatch,
    Verdict,
};
use laser_sdk::iggy::prelude::{HeaderKey, HeaderValue, IggyError};
use laser_sdk::prelude::{Laser, ProducerMessage, Routing};
use laser_sdk::wire::headers::CONTENT_TYPE;
use serde::Serialize;
use serde_json::Value;

const MIXED_TOPIC: &str = "mixed";
const EVENT_TYPE: &str = "event.type";
const PREVIEW_RECORDS: u32 = 16;

/// What one filter did with each record of the mixed log.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MixedVerdicts {
    pub filter: String,
    pub selected: Vec<u64>,
    pub rejected: Vec<u64>,
    pub unevaluated: Vec<(u64, String)>,
}

/// One log, many kinds of events: publish fleet, depot, and billing events in three codecs into
/// one partition, then show what each filter selects, skips, and hands over unevaluated.
pub async fn mixed_log(
    laser: &Laser,
    provisioned: &Provisioned,
) -> Result<(Vec<MixedVerdicts>, Vec<String>), DemoError> {
    let stream = provisioned.run_file.run_id.stream();
    let producer = laser
        .stream(&stream)
        .topic(MIXED_TOPIC)
        .producer()
        .create_stream(false)
        .create_topic(true)
        .partitions(1)
        .build()
        .await?;
    let messages = records()?
        .into_iter()
        .map(|(payload, headers)| ProducerMessage::new(payload).with_headers(headers));
    producer
        .send_batch_with_routing(messages, Some(Routing::Partition(0)))
        .await?;
    let mut verdicts = Vec::new();
    let mut lines = Vec::new();
    for (index, (name, filter)) in filters().into_iter().enumerate() {
        let group = laser
            .stream(&stream)
            .topic(MIXED_TOPIC)
            .consumer_group(format!("mixed-{index}"));
        let configured = group.create().filter(filter.clone()).build().await?;
        let binding = configured.filter.expect("requested policy");
        let preview = group
            .filter()
            .preview(0)
            .await?
            .max_records(PREVIEW_RECORDS)
            .explain(true)
            .send()
            .await?;
        let mut outcome = MixedVerdicts {
            filter: name.to_owned(),
            selected: Vec::new(),
            rejected: Vec::new(),
            unevaluated: Vec::new(),
        };
        for record in &preview.records {
            match record.verdict {
                Verdict::Selected => outcome.selected.push(record.offset),
                Verdict::Rejected => outcome.rejected.push(record.offset),
                Verdict::Fault => {
                    let reason = record.fault.unwrap_or(FaultReason::Malformed);
                    let record_policy = if reason.is_foreign() {
                        Some(filter.foreign_policy)
                    } else if reason == FaultReason::TypeMismatch {
                        Some(filter.mismatch_policy)
                    } else {
                        None
                    };
                    let policy = match record_policy {
                        Some(RecordPolicy::Pass) => FaultPolicy::Pass,
                        Some(RecordPolicy::Reject) => FaultPolicy::Drop,
                        None => filter.fault_policy,
                    };
                    match policy {
                        FaultPolicy::Pass => outcome
                            .unevaluated
                            .push((record.offset, reason.to_string())),
                        FaultPolicy::Drop => outcome.rejected.push(record.offset),
                        FaultPolicy::Stop => {
                            return Err(DemoError::Incomplete(format!(
                                "Filter {name} stopped at mixed offset {}: {reason}",
                                record.offset
                            )));
                        }
                    }
                }
            }
        }
        let mut reader = group.reader()?.start(FilteredStart::First).build().await?;
        let mut unevaluated = Vec::new();
        let read = async {
            while let Some(page) = reader.try_next_page().await? {
                unevaluated.extend(
                    page.records
                        .iter()
                        .filter(|record| !record.evaluated)
                        .map(|record| record.offset),
                );
                reader.ack_page(&page).await?;
            }
            Ok::<(), DemoError>(())
        }
        .await;
        let closed = reader.close().await.map_err(DemoError::from);
        let retired = crate::cleanup::retire_binding(laser, &binding)
            .await
            .map_err(DemoError::from);
        read.and(closed).and(retired)?;
        if unevaluated
            != outcome
                .unevaluated
                .iter()
                .map(|(offset, _)| *offset)
                .collect::<Vec<_>>()
        {
            return Err(DemoError::Incomplete(format!(
                "Filter {name} delivered different unevaluated offsets from its preview"
            )));
        }
        if !unevaluated.is_empty() {
            act(&format!(
                "Reader {name} received unevaluated offsets {unevaluated:?} with their original payloads."
            ));
        }
        let line = narrate(&outcome);
        act(&line);
        lines.push(line);
        verdicts.push(outcome);
    }
    Ok((verdicts, lines))
}

// Offsets 0 to 4: a north pharma reading, a depot event in CBOR, an invoice in Protobuf, a reading
// whose region is a number instead of text, and a depot event in JSON.
fn records() -> Result<Vec<(Vec<u8>, Headers)>, DemoError> {
    let reading = Codec::Json.encode(&fixtures::reefer_fault(), &SchemaSet::default())?;
    let mut broken: Value = serde_json::from_slice(&reading)
        .map_err(|error| DemoError::Incomplete(error.to_string()))?;
    broken["region"] = Value::from(7);
    let broken =
        serde_json::to_vec(&broken).map_err(|error| DemoError::Incomplete(error.to_string()))?;
    let mut depot = Vec::new();
    ciborium::into_writer(
        &serde_json::json!({ "dock": 4, "state": "opened" }),
        &mut depot,
    )
    .map_err(|error| DemoError::Incomplete(error.to_string()))?;
    Ok(vec![
        (
            reading,
            headers("fleet.truck.v1.reading", Codec::Json.content_type().code())?,
        ),
        (
            depot,
            headers("depot.dock.v1.opened", Codec::Cbor.content_type().code())?,
        ),
        (
            b"\x08\x96\x01\x12\x07INV-042".to_vec(),
            headers(
                "billing.invoice.v2.issued",
                Codec::Protobuf.content_type().code(),
            )?,
        ),
        (
            broken,
            headers("fleet.truck.v1.reading", Codec::Json.content_type().code())?,
        ),
        (
            br#"{"dock":2,"state":"closed"}"#.to_vec(),
            headers("depot.dock.v1.closed", Codec::Json.content_type().code())?,
        ),
    ])
}

fn headers(event_type: &str, content_type: u8) -> Result<Headers, DemoError> {
    let invalid = |error: IggyError| DemoError::Incomplete(error.to_string());
    Ok(Headers::from([
        (
            HeaderKey::try_from(EVENT_TYPE).map_err(invalid)?,
            HeaderValue::try_from(event_type).map_err(invalid)?,
        ),
        (
            HeaderKey::try_from(CONTENT_TYPE).map_err(invalid)?,
            HeaderValue::from(content_type),
        ),
    ]))
}

fn filters() -> Vec<(&'static str, ConsumerFilter)> {
    let regional = RolePolicy::for_role(Role::Regional).filter(Codec::Json, &[]);
    vec![
        ("north-pharma", regional.clone()),
        (
            "north-pharma with mismatch pass",
            regional.with_mismatch_policy(RecordPolicy::Pass),
        ),
        (
            "depot v1 by glob",
            ConsumerFilter::headers_only(FilterExpr::header_text(
                EVENT_TYPE,
                TextMatch::Glob,
                "depot.*.v1.*",
            )),
        ),
        (
            "every v1 event ignoring case",
            ConsumerFilter::headers_only(
                FilterExpr::header_text(EVENT_TYPE, TextMatch::Contains, ".V1.").case_insensitive(),
            ),
        ),
    ]
}

fn narrate(outcome: &MixedVerdicts) -> String {
    let offsets = |offsets: &[u64]| {
        offsets
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut line = format!(
        "Filter {} over the mixed log selected [{}] and skipped [{}]",
        outcome.filter,
        offsets(&outcome.selected),
        offsets(&outcome.rejected)
    );
    if !outcome.unevaluated.is_empty() {
        let handed: Vec<String> = outcome
            .unevaluated
            .iter()
            .map(|(offset, reason)| format!("{offset} ({reason})"))
            .collect();
        line.push_str(&format!(
            ", and handed over unevaluated [{}]",
            handed.join(", ")
        ));
    }
    line.push('.');
    line
}
