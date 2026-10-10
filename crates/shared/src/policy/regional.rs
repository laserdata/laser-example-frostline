use crate::domain::{Cargo, Region};
use crate::event::{EventKind, FleetEvent};
use laser_sdk::filters::{Coerce, FilterExpr};
use laser_sdk::query::{CmpOp, TypedValue};

const HEAVY_LOAD_TONNES: &str = "10.00";

pub fn expression() -> FilterExpr {
    FilterExpr::all([
        FilterExpr::pred("region", CmpOp::Eq, Region::North.to_string()),
        FilterExpr::pred(
            "cargo",
            CmpOp::In,
            TypedValue::List(vec![
                TypedValue::from(Cargo::Pharma.to_string()),
                TypedValue::from(Cargo::Frozen.to_string()),
            ]),
        ),
        FilterExpr::any([
            FilterExpr::pred("kind", CmpOp::Eq, EventKind::Telemetry.to_string()),
            FilterExpr::all([
                FilterExpr::pred("kind", CmpOp::Eq, EventKind::Change.to_string()),
                FilterExpr::pred_as(
                    "change.after.declared_weight_tonnes",
                    CmpOp::Gte,
                    HEAVY_LOAD_TONNES,
                    Coerce::Number,
                ),
            ]),
        ]),
    ])
}

pub fn oracle(event: &FleetEvent) -> bool {
    let in_slice = event.region == Some(Region::North)
        && matches!(event.cargo, Some(Cargo::Pharma | Cargo::Frozen));
    let heavy = || {
        event
            .change
            .as_ref()
            .and_then(|change| change.after.as_ref())
            .is_some_and(|after| {
                hundredths(&after.declared_weight_tonnes) >= hundredths(HEAVY_LOAD_TONNES)
            })
    };
    in_slice
        && match event.kind {
            EventKind::Telemetry => true,
            EventKind::Change => heavy(),
            EventKind::Checkpoint => false,
        }
}

// Weights are written with two decimals, so comparing hundredths is exact.
fn hundredths(weight: &str) -> Option<u64> {
    let (whole, fraction) = weight.split_once('.')?;
    let fraction = format!("{fraction:0<2}");
    Some(whole.parse::<u64>().ok()? * 100 + fraction.get(..2)?.parse::<u64>().ok()?)
}
