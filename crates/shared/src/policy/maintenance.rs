use crate::event::FleetEvent;
use crate::names::{Header, Severity, Unit};
use laser_sdk::filters::FilterExpr;
use laser_sdk::query::CmpOp;

pub fn expression() -> FilterExpr {
    FilterExpr::all([
        FilterExpr::header(Header::Unit.name(), CmpOp::Eq, Unit::Reefer.to_string()),
        FilterExpr::header(Header::Severity.name(), CmpOp::Gte, Severity::Error as i32),
    ])
}

pub fn oracle(event: &FleetEvent) -> bool {
    let (unit, severity) = event.diagnosis();
    unit == Unit::Reefer && severity >= Severity::Error
}
