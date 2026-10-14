use super::cases::Case;
use super::{BATCH_RECORDS, MATCH_HEADER, MATCHED, SENTINEL, TOPIC};
use crate::error::BenchError;
use frostline_shared::LaserFactory;
use laser_sdk::iggy::prelude::{HeaderKey, HeaderValue};
use laser_sdk::prelude::{ProducerMessage, Routing};

const BATCH_BYTES: usize = 4 * 1024 * 1024;

pub async fn publish(factory: &LaserFactory, stream: &str, case: Case) -> Result<(), BenchError> {
    let laser = factory.connect(stream).await?;
    let producer = laser
        .stream(stream)
        .topic(TOPIC)
        .producer()
        .partitions(1)
        .build()
        .await?;
    let selected = payload(true, case.matched_bytes)?;
    let rejected = payload(false, case.other_bytes)?;
    let key = HeaderKey::try_from(MATCH_HEADER)
        .map_err(|error| BenchError::Invalid(error.to_string()))?;
    let mut index = 0;
    while index <= case.records {
        let mut batch = Vec::new();
        let mut bytes = 0;
        while index <= case.records && batch.len() < BATCH_RECORDS as usize {
            let matched = case.matched(index);
            let value = if matched { &selected } else { &rejected };
            if !batch.is_empty() && bytes + value.len() > BATCH_BYTES {
                break;
            }
            let marker = if index == case.records {
                SENTINEL
            } else if matched {
                MATCHED
            } else {
                0
            };
            batch.push(
                ProducerMessage::new(value.clone())
                    .with_headers([(key.clone(), HeaderValue::from(marker))].into()),
            );
            bytes += value.len();
            index += 1;
        }
        producer
            .send_batch_with_routing(batch, Some(Routing::Partition(0)))
            .await?;
    }
    laser.close().await?;
    Ok(())
}

fn payload(selected: bool, bytes: usize) -> Result<Vec<u8>, BenchError> {
    let mut value = serde_json::json!({"selected": selected, "nested": {"selected": selected},
        "amount": if selected { "12.50" } else { "0.25" },
        "event": if selected { "selected.change" } else { "rejected.change" }, "padding": ""});
    if !selected {
        value["excluded"] = serde_json::json!(true);
    }
    let overhead = serde_json::to_vec(&value)?.len();
    let padding = bytes.checked_sub(overhead).ok_or_else(|| {
        BenchError::Invalid("dataset payload is smaller than its fields".to_owned())
    })?;
    value["padding"] = serde_json::json!("x".repeat(padding));
    Ok(serde_json::to_vec(&value)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use laser_sdk::filters::Verdict;
    use laser_sdk::wire::filter::eval::{CompiledFilter, DecodeLimits, FilterRecord};

    #[test]
    fn given_each_payload_case_when_decoded_then_should_match_its_oracle_and_exact_byte_size() {
        for case in super::super::cases::MATRIX {
            if matches!(
                case.predicate,
                super::super::cases::Predicate::Header | super::super::cases::Predicate::Boolean
            ) {
                continue;
            }
            let compiled = CompiledFilter::compile(&case.filter()).expect("filter compiles");
            for selected in [false, true] {
                let size = if selected {
                    case.matched_bytes
                } else {
                    case.other_bytes
                };
                let bytes = payload(selected, size).expect("payload");
                assert_eq!(bytes.len(), size, "{}", case.name);
                assert_eq!(
                    super::super::oracle::matches(case.predicate, &bytes, selected)
                        .expect("typed predicate"),
                    selected
                );
                let verdict = compiled.evaluate(
                    &FilterRecord {
                        payload: &bytes,
                        headers: &[],
                    },
                    &DecodeLimits::default(),
                );
                assert_eq!(
                    verdict,
                    if selected {
                        Verdict::Selected
                    } else {
                        Verdict::Rejected
                    },
                    "{}",
                    case.name
                );
            }
        }
    }
}
