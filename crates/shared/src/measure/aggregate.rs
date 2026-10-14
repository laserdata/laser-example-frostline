use super::{Receipt, Report, Subscription, Totals, WindowManifest};
use crate::domain::WindowId;
use crate::names::RunId;
use std::collections::BTreeMap;
use thiserror::Error;

/// Joins window manifests and receipts, and decides which windows every subscription has completed.
#[derive(Debug)]
pub struct Aggregator {
    run_id: RunId,
    partitions: Vec<u32>,
    subscriptions: Vec<Subscription>,
    max_pending: usize,
    windows: BTreeMap<WindowId, WindowState>,
    totals: Totals,
    expired: u64,
    retired_through: Option<WindowId>,
}

#[derive(Debug, Default)]
pub struct WindowState {
    pub manifest: Option<WindowManifest>,
    pub receipts: BTreeMap<(Subscription, u32), Receipt>,
    pub conflicting: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Completion {
    Complete,
    Pending { missing: usize },
    Conflicting,
    Mismatched { group: String, partition_id: u32 },
}

#[derive(Debug, Error, PartialEq)]
pub enum AggregateError {
    #[error("report of run {got} arrived for run {expected}")]
    ForeignRun { expected: RunId, got: RunId },
}

impl Aggregator {
    pub fn new(
        run_id: RunId,
        partitions: Vec<u32>,
        subscriptions: Vec<Subscription>,
        max_pending: usize,
    ) -> Self {
        Self {
            run_id,
            partitions,
            subscriptions,
            max_pending,
            windows: BTreeMap::new(),
            totals: Totals::default(),
            expired: 0,
            retired_through: None,
        }
    }

    pub fn apply(&mut self, report: Report) -> Result<(), AggregateError> {
        let (run_id, window_id) = match &report {
            Report::Manifest(manifest) => (&manifest.run_id, manifest.window_id),
            Report::Receipt(receipt) => (&receipt.run_id, receipt.window_id),
        };
        if *run_id != self.run_id {
            return Err(AggregateError::ForeignRun {
                expected: self.run_id.clone(),
                got: run_id.clone(),
            });
        }
        if self
            .retired_through
            .is_some_and(|latest| window_id <= latest)
        {
            return Ok(());
        }
        if let Report::Receipt(receipt) = &report
            && (!self.subscriptions.contains(&receipt.subscription())
                || !self.partitions.contains(&receipt.partition_id))
        {
            return Ok(());
        }
        let window = self.windows.entry(window_id).or_default();
        match report {
            Report::Manifest(manifest) => match &window.manifest {
                Some(existing) if *existing != manifest => window.conflicting = true,
                Some(_) => {}
                None => window.manifest = Some(manifest),
            },
            Report::Receipt(receipt) => {
                let slot = (receipt.subscription(), receipt.partition_id);
                match window.receipts.get(&slot) {
                    Some(existing) if !existing.same_content(&receipt) => window.conflicting = true,
                    Some(_) => {}
                    None => {
                        window.receipts.insert(slot, receipt);
                    }
                }
            }
        }
        self.fold_completed();
        self.evict();
        self.fold_completed();
        Ok(())
    }

    pub fn completion(&self, window_id: WindowId) -> Completion {
        let Some(window) = self.windows.get(&window_id) else {
            return Completion::Pending {
                missing: self.subscriptions.len() * self.partitions.len() + 1,
            };
        };
        if window.conflicting {
            return Completion::Conflicting;
        }
        let Some(manifest) = &window.manifest else {
            return Completion::Pending { missing: 1 };
        };
        let mut missing = 0;
        for subscription in &self.subscriptions {
            for &partition_id in &self.partitions {
                let Some(receipt) = window.receipts.get(&(subscription.clone(), partition_id))
                else {
                    missing += 1;
                    continue;
                };
                let expected = manifest.expected(&subscription.group, partition_id);
                if expected.is_none_or(|expected| {
                    expected.digest != receipt.digest
                        || expected.matches != receipt.matches
                        || expected.payload_bytes != receipt.matched_bytes
                }) {
                    return Completion::Mismatched {
                        group: subscription.group.clone(),
                        partition_id,
                    };
                }
            }
        }
        match missing {
            0 => Completion::Complete,
            missing => Completion::Pending { missing },
        }
    }

    /// Totals over the contiguous windows every subscription completed.
    pub fn totals(&self) -> &Totals {
        &self.totals
    }

    /// The oldest window still waiting, and why.
    pub fn first_pending(&self) -> Option<(WindowId, Completion)> {
        self.windows.keys().next().map(|window_id| {
            let expected = WindowId(
                self.totals
                    .latest
                    .map_or(0, |latest| latest.0.saturating_add(1)),
            );
            let pending = if self.expired == 0 {
                (*window_id).min(expected)
            } else {
                *window_id
            };
            (pending, self.completion(pending))
        })
    }

    /// The newest window the producer confirmed, open or already folded.
    pub fn producer_window(&self) -> Option<WindowId> {
        self.windows
            .iter()
            .rev()
            .find(|(_, window)| window.manifest.is_some())
            .map(|(window_id, _)| *window_id)
            .or(self.totals.latest)
    }

    pub fn expired(&self) -> u64 {
        self.expired
    }

    // Windows complete in order, so a finished prefix folds into the totals and leaves memory.
    fn fold_completed(&mut self) {
        while let Some(&oldest) = self.windows.keys().next()
            && oldest.0
                == self
                    .retired_through
                    .map_or(0, |latest| latest.0.saturating_add(1))
            && self.completion(oldest) == Completion::Complete
        {
            let mut window = self
                .windows
                .remove(&oldest)
                .expect("the oldest window exists");
            window.receipts.retain(|(subscription, partition), _| {
                self.subscriptions.contains(subscription) && self.partitions.contains(partition)
            });
            self.totals.fold(oldest, &window);
            self.retired_through = Some(oldest);
        }
    }

    // Past the bound, the oldest unresolved window is dropped and counted, never reported as complete.
    fn evict(&mut self) {
        while self.windows.len() > self.max_pending {
            let oldest = *self
                .windows
                .keys()
                .next()
                .expect("a window exists past the bound");
            self.windows.remove(&oldest);
            self.retired_through = Some(oldest);
            self.expired += 1;
        }
    }
}

#[cfg(test)]
#[path = "ordering_tests.rs"]
mod ordering_tests;
