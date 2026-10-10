use frostline_shared::event::FleetEvent;
use std::collections::BTreeMap;

/// Where a handled record came from, so narration can show its original position.
#[derive(Clone, Copy, Debug)]
pub struct Delivered {
    pub partition: u32,
    pub offset: u64,
}

/// One team's business logic. It is synchronous: the reader loop owns every network call.
pub trait Handler: Send {
    fn handle(&mut self, event: &FleetEvent, delivered: Delivered);

    /// The sampled business line for a handled record, in plain words.
    fn narrate(&self, event: &FleetEvent, delivered: Delivered) -> Option<String>;

    /// One line on what the team knows now, for the final report.
    fn status(&self) -> String;
}

/// A map that drops its oldest entry past a bound, so a team's view never outgrows the fleet.
#[derive(Debug)]
pub struct BoundedTable<K, V> {
    entries: BTreeMap<K, (u64, V)>,
    bound: usize,
    clock: u64,
}

impl<K: Ord + Clone, V> BoundedTable<K, V> {
    pub fn new(bound: usize) -> Self {
        Self {
            entries: BTreeMap::new(),
            bound: bound.max(1),
            clock: 0,
        }
    }

    pub fn insert(&mut self, key: K, value: V) {
        self.clock += 1;
        self.entries.insert(key, (self.clock, value));
        if self.entries.len() > self.bound
            && let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, (stamp, _))| *stamp)
                .map(|(key, _)| key.clone())
        {
            self.entries.remove(&oldest);
        }
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.entries.remove(key).map(|(_, value)| value)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_more_entries_than_the_bound_when_inserted_then_should_drop_the_oldest() {
        let mut table = BoundedTable::new(2);
        table.insert("a", 1);
        table.insert("b", 2);
        table.insert("c", 3);
        assert_eq!(table.len(), 2);
        assert_eq!(table.remove(&"a"), None);
        assert_eq!(table.remove(&"c"), Some(3));
    }
}
