use crate::domain::{Cargo, Field, Operation, TemperatureBand};
use crate::event::{EventKind, FleetEvent};
use laser_sdk::filters::FilterExpr;
use laser_sdk::query::{CmpOp, TypedValue};

pub fn expression() -> FilterExpr {
    FilterExpr::all([
        FilterExpr::pred("kind", CmpOp::Eq, EventKind::Change.to_string()),
        FilterExpr::any([
            FilterExpr::all([
                FilterExpr::pred("change.op", CmpOp::Eq, Operation::Update.to_string()),
                FilterExpr::pred(
                    "change.changed",
                    CmpOp::Contains,
                    Field::TemperatureBand.to_string(),
                ),
                FilterExpr::pred(
                    "change.before.temperature_band",
                    CmpOp::Ne,
                    TemperatureBand::Unsafe.to_string(),
                ),
                FilterExpr::pred(
                    "change.after.temperature_band",
                    CmpOp::Eq,
                    TemperatureBand::Unsafe.to_string(),
                ),
            ]),
            FilterExpr::all([
                FilterExpr::pred("change.op", CmpOp::Eq, Operation::Delete.to_string()),
                FilterExpr::pred("cargo", CmpOp::In, refrigerated()),
            ]),
        ]),
    ])
}

pub fn oracle(event: &FleetEvent) -> bool {
    let Some(change) = &event.change else {
        return false;
    };
    match change.op {
        Operation::Update => {
            change.changed.contains(&Field::TemperatureBand) && change.enters_unsafe()
        }
        Operation::Delete => matches!(event.cargo, Some(Cargo::Pharma | Cargo::Frozen)),
        Operation::Insert => false,
    }
}

pub fn current_expression() -> FilterExpr {
    FilterExpr::all([
        FilterExpr::pred("kind", CmpOp::Eq, EventKind::Change.to_string()),
        FilterExpr::pred("change.op", CmpOp::Eq, Operation::Update.to_string()),
        FilterExpr::pred(
            "change.after.temperature_band",
            CmpOp::Eq,
            TemperatureBand::Unsafe.to_string(),
        ),
    ])
}

pub fn current_oracle(event: &FleetEvent) -> bool {
    event.change.as_ref().is_some_and(|change| {
        change.op == Operation::Update
            && change
                .after
                .as_ref()
                .is_some_and(|after| after.temperature_band == TemperatureBand::Unsafe)
    })
}

fn refrigerated() -> TypedValue {
    TypedValue::List(
        [Cargo::Pharma, Cargo::Frozen]
            .iter()
            .map(|cargo| TypedValue::from(cargo.to_string()))
            .collect(),
    )
}
