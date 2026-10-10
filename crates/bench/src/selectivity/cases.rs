use laser_sdk::filters::{Coerce, ConsumerFilter, FilterExpr, TextMatch};
use laser_sdk::query::CmpOp;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Case {
    pub name: &'static str,
    pub per_mille: u32,
    pub records: u64,
    pub matched_bytes: usize,
    pub other_bytes: usize,
    pub predicate: Predicate,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Predicate {
    Header,
    Shallow,
    Nested,
    Missing,
    Coerced,
    Text,
    Boolean,
}

pub const DENSITIES: [Case; 5] = [
    Case::uniform("selectivity_0", 0, 1024),
    Case::uniform("selectivity_1", 1, 1024),
    Case::uniform("selectivity_10", 10, 1024),
    Case::uniform("selectivity_100", 100, 1024),
    Case::uniform("selectivity_1000", 1000, 1024),
];

pub const MATRIX: [Case; 12] = [
    Case::payload("payload_256", 256),
    Case::payload("payload_1k", 1024),
    Case {
        records: 8192,
        ..Case::payload("payload_16k", 16 * 1024)
    },
    Case {
        records: 128,
        ..Case::payload("payload_near_cap", 1024 * 1024 - 1024)
    },
    Case {
        matched_bytes: 256,
        other_bytes: 4096,
        ..Case::payload("matched_smaller", 1024)
    },
    Case {
        matched_bytes: 4096,
        other_bytes: 256,
        ..Case::payload("matched_larger", 1024)
    },
    Case::uniform("predicate_headers", 10, 1024),
    Case {
        predicate: Predicate::Nested,
        ..Case::payload("predicate_nested", 1024)
    },
    Case {
        predicate: Predicate::Missing,
        ..Case::payload("predicate_missing", 1024)
    },
    Case {
        predicate: Predicate::Coerced,
        ..Case::payload("predicate_coerced", 1024)
    },
    Case {
        predicate: Predicate::Text,
        ..Case::payload("predicate_text", 1024)
    },
    Case {
        predicate: Predicate::Boolean,
        ..Case::payload("predicate_boolean", 1024)
    },
];

impl Case {
    const fn uniform(name: &'static str, per_mille: u32, bytes: usize) -> Self {
        Self {
            name,
            per_mille,
            records: 200_000,
            matched_bytes: bytes,
            other_bytes: bytes,
            predicate: Predicate::Header,
        }
    }

    const fn payload(name: &'static str, bytes: usize) -> Self {
        Self {
            predicate: Predicate::Shallow,
            ..Self::uniform(name, 10, bytes)
        }
    }

    pub fn find(name: &str) -> Option<Self> {
        DENSITIES
            .iter()
            .chain(&MATRIX)
            .find(|case| case.name == name)
            .copied()
    }

    pub fn matched(self, index: u64) -> bool {
        self.per_mille > 0 && index.is_multiple_of(u64::from(1000 / self.per_mille))
    }

    pub fn matches(self) -> u64 {
        1000_u32
            .checked_div(self.per_mille)
            .map_or(0, |spacing| self.records.div_ceil(u64::from(spacing)))
    }

    pub fn source_bytes(self) -> u64 {
        self.matches() * self.matched_bytes as u64
            + (self.records - self.matches()) * self.other_bytes as u64
    }

    pub fn filter(self) -> ConsumerFilter {
        let header =
            || FilterExpr::header(super::MATCH_HEADER, CmpOp::Eq, i32::from(super::MATCHED));
        let payload = match self.predicate {
            Predicate::Header => header(),
            Predicate::Shallow => FilterExpr::pred("selected", CmpOp::Eq, true),
            Predicate::Nested => FilterExpr::pred("nested.selected", CmpOp::Eq, true),
            Predicate::Missing => FilterExpr::absent("excluded"),
            Predicate::Coerced => FilterExpr::pred_as("amount", CmpOp::Gt, "10.00", Coerce::Number),
            Predicate::Text => FilterExpr::text("event", TextMatch::Glob, "selected.*"),
            Predicate::Boolean => FilterExpr::all([
                header(),
                FilterExpr::pred("nested.selected", CmpOp::Eq, true),
            ]),
        };
        let expr = FilterExpr::any([
            FilterExpr::header(super::MATCH_HEADER, CmpOp::Eq, i32::from(super::SENTINEL)),
            payload,
        ]);
        match self.predicate {
            Predicate::Header => ConsumerFilter::headers_only(expr),
            _ => ConsumerFilter::json(expr),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_partial_density_period_when_counted_then_should_include_the_first_match() {
        let case = Case {
            records: 128,
            ..MATRIX[0]
        };
        assert_eq!(case.matches(), 2);
        assert_eq!(
            (0..case.records)
                .filter(|index| case.matched(*index))
                .count(),
            2
        );
        assert_eq!(DENSITIES[0].matches(), 0);
        assert_eq!(DENSITIES[4].matches(), 200_000);
    }
}
