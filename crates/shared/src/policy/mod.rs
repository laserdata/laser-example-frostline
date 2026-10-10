mod maintenance;
mod regional;
mod safety;

use crate::codec::Codec;
use crate::event::FleetEvent;
use crate::names::{Frame, Header, Role, SAFETY_CURRENT_GROUP};
use laser_sdk::filters::{ConsumerFilter, FilterExpr};
use laser_sdk::query::CmpOp;

/// One team's selection: the filter the server runs and the hand-written predicate that checks it.
#[derive(Clone, Debug)]
pub struct RolePolicy {
    pub role: Role,
    pub name: &'static str,
    pub description: &'static str,
    pub expression: FilterExpr,
    headers_only: bool,
    oracle: fn(&FleetEvent) -> bool,
}

impl RolePolicy {
    pub fn for_role(role: Role) -> Self {
        match role {
            Role::FoodSafety => Self {
                role,
                name: "food-safety",
                description: "Trucks entering an unsafe temperature band, and refrigerated trucks leaving the fleet",
                expression: safety::expression(),
                headers_only: false,
                oracle: safety::oracle,
            },
            Role::Maintenance => Self {
                role,
                name: "maintenance",
                description: "Refrigeration unit faults at error severity or above, read from headers only",
                expression: maintenance::expression(),
                headers_only: true,
                oracle: maintenance::oracle,
            },
            Role::Regional => Self {
                role,
                name: "north-pharma",
                description: "Telemetry from pharma and frozen trucks in the north, and their changes when the load is 10 tonnes or more",
                expression: regional::expression(),
                headers_only: false,
                oracle: regional::oracle,
            },
        }
    }

    /// The A/B revision of food safety: every update whose current band is unsafe.
    pub fn safety_current() -> Self {
        Self {
            role: Role::FoodSafety,
            name: "food-safety",
            description: "Every update of a truck whose current band is unsafe",
            expression: safety::current_expression(),
            headers_only: false,
            oracle: safety::current_oracle,
        }
    }

    /// The server filter. Every reader also accepts checkpoints, decided by the frame header before any decode.
    pub fn filter(&self, codec: Codec, schema_refs: &[u32]) -> ConsumerFilter {
        let expr = FilterExpr::any([checkpoint_branch(), self.expression.clone()]);
        if self.headers_only {
            ConsumerFilter::headers_only(expr)
        } else {
            codec.wrap(expr, schema_refs)
        }
    }

    /// Whether this team wants `event`. Checkpoints are never a business match.
    pub fn matches(&self, event: &FleetEvent) -> bool {
        event.frame() == Frame::Business && (self.oracle)(event)
    }
}

/// Every group a run measures, with the policy it reads: the three teams and the A/B revision.
pub fn by_group() -> Vec<(&'static str, RolePolicy)> {
    vec![
        (
            Role::FoodSafety.group(),
            RolePolicy::for_role(Role::FoodSafety),
        ),
        (SAFETY_CURRENT_GROUP, RolePolicy::safety_current()),
        (
            Role::Maintenance.group(),
            RolePolicy::for_role(Role::Maintenance),
        ),
        (Role::Regional.group(), RolePolicy::for_role(Role::Regional)),
    ]
}

pub fn checkpoint_branch() -> FilterExpr {
    FilterExpr::header(Header::Frame.name(), CmpOp::Eq, Frame::Checkpoint as i32)
}

#[cfg(test)]
mod tests;
