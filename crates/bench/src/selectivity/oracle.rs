use super::cases::Predicate;
use crate::error::BenchError;
use laser_sdk::wire::filter::ExactDecimal;
use serde::Deserialize;
use std::sync::LazyLock;

static MIN_AMOUNT: LazyLock<ExactDecimal> =
    LazyLock::new(|| ExactDecimal::parse("10.00").expect("decimal threshold"));

#[derive(Deserialize)]
struct Record {
    selected: bool,
    nested: Nested,
    amount: String,
    event: String,
    excluded: Option<bool>,
}

#[derive(Deserialize)]
struct Nested {
    selected: bool,
}

/// An ordinary application's typed predicate, independent from the filter evaluator.
pub fn matches(kind: Predicate, payload: &[u8], header_match: bool) -> Result<bool, BenchError> {
    if matches!(kind, Predicate::Header) {
        return Ok(header_match);
    }
    if matches!(kind, Predicate::Boolean) && !header_match {
        return Ok(false);
    }
    let record: Record = serde_json::from_slice(payload)?;
    Ok(match kind {
        Predicate::Header => header_match,
        Predicate::Shallow => record.selected,
        Predicate::Nested | Predicate::Boolean => record.nested.selected,
        Predicate::Missing => record.excluded.is_none(),
        Predicate::Coerced => {
            ExactDecimal::parse(&record.amount).is_some_and(|value| value.cmp(&MIN_AMOUNT).is_gt())
        }
        Predicate::Text => record.event.starts_with("selected."),
    })
}
