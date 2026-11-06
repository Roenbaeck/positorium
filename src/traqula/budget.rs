//! Deterministic query-work accounting and RAII workspace reservations.
//! Workspace is an explicit accounting model, not process RSS or allocator usage.

use crate::error::DatabaseError;
use std::cell::Cell;
use std::ops::Deref;
use std::rc::Rc;

/// Optional limits shared by all searches in one script. `None` is unlimited;
/// zero is a valid budget. Parsing, database storage, mutation staging, and
/// caller-owned result buffers are outside the query workspace accounting model.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct QueryLimits {
    /// Cumulative logical work units, including batched index and sort work.
    pub max_work_units: Option<u64>,
    /// Maximum simultaneously reserved query workspace bytes.
    pub max_memory_bytes: Option<u64>,
}

#[derive(Default)]
pub(super) struct QueryBudget(Option<Rc<State>>);

struct State {
    limits: QueryLimits,
    work: Cell<u64>,
    memory: Cell<u64>,
    peak: Cell<u64>,
}

impl QueryBudget {
    pub fn tracks_memory(&self) -> bool {
        self.0
            .as_ref()
            .is_some_and(|state| state.limits.max_memory_bytes.is_some())
    }
    pub fn new(limits: QueryLimits) -> Self {
        Self((limits != QueryLimits::default()).then(|| {
            Rc::new(State {
                limits,
                work: Cell::new(0),
                memory: Cell::new(0),
                peak: Cell::new(0),
            })
        }))
    }

    pub fn work(&self, units: u64) -> Result<(), DatabaseError> {
        if let Some(state) = &self.0
            && let Some(limit) = state.limits.max_work_units
        {
            let used = state
                .work
                .get()
                .checked_add(units)
                .filter(|&used| used <= limit)
                .ok_or(DatabaseError::ResourceLimit {
                    resource: "query work units",
                    limit,
                })?;
            state.work.set(used);
        }
        Ok(())
    }

    pub fn memory(&self, bytes: u64) -> Result<Reservation, DatabaseError> {
        let mut reservation = Reservation {
            state: self
                .0
                .as_ref()
                .filter(|s| s.limits.max_memory_bytes.is_some())
                .cloned(),
            bytes: 0,
        };
        reservation.resize(bytes)?;
        Ok(reservation)
    }
}

#[derive(Default)]
pub(super) struct Reservation {
    state: Option<Rc<State>>,
    bytes: u64,
}

impl std::fmt::Debug for Reservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reservation")
            .field("bytes", &self.bytes)
            .finish()
    }
}

impl Reservation {
    pub fn active(&self) -> bool {
        self.state.is_some()
    }
    #[cfg(test)]
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
    pub fn resize(&mut self, bytes: u64) -> Result<(), DatabaseError> {
        let Some(state) = &self.state else {
            return Ok(());
        };
        let limit = state.limits.max_memory_bytes.unwrap();
        let total = state
            .memory
            .get()
            .checked_sub(self.bytes)
            .and_then(|other| other.checked_add(bytes))
            .filter(|&total| total <= limit)
            .ok_or(DatabaseError::ResourceLimit {
                resource: "query memory bytes",
                limit,
            })?;
        state.memory.set(total);
        state.peak.set(state.peak.get().max(total));
        self.bytes = bytes;
        Ok(())
    }
    pub fn grow(&mut self, bytes: u64) -> Result<(), DatabaseError> {
        if self.active() {
            let total = self
                .bytes
                .checked_add(bytes)
                .ok_or(DatabaseError::ResourceLimit {
                    resource: "query memory bytes",
                    limit: self
                        .state
                        .as_ref()
                        .unwrap()
                        .limits
                        .max_memory_bytes
                        .unwrap(),
                })?;
            self.resize(total)?;
        }
        Ok(())
    }
    fn release(&mut self, bytes: u64) {
        if self.active() {
            self.resize(self.bytes - bytes)
                .expect("releasing reserved memory");
        }
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if let Some(state) = &self.state {
            state.memory.set(state.memory.get() - self.bytes);
        }
    }
}

/// Accounted owned heap payload beyond a value's inline size. Shared store
/// objects behind Arcs are excluded; copied strings and container nodes count.
pub(super) trait Workspace {
    fn heap_bytes(&self) -> u64;
}

impl<T: Workspace> Workspace for Vec<T> {
    fn heap_bytes(&self) -> u64 {
        (self.capacity() as u64)
            .saturating_mul(size_of::<T>() as u64)
            .saturating_add(
                self.iter()
                    .map(Workspace::heap_bytes)
                    .fold(0, u64::saturating_add),
            )
    }
}

impl<A: Workspace, B: Workspace> Workspace for (A, B) {
    fn heap_bytes(&self) -> u64 {
        self.0.heap_bytes().saturating_add(self.1.heap_bytes())
    }
}

impl<T> Workspace for std::sync::Arc<T> {
    fn heap_bytes(&self) -> u64 {
        0
    }
}

/// A vector whose capacity and owned payload remain reserved through moves and
/// iteration. Reservations are checked before growth, not after materialization.
pub(super) struct BudgetVec<T: Workspace> {
    values: Vec<T>,
    memory: Reservation,
    payload: u64,
}

impl<T: Workspace> BudgetVec<T> {
    pub fn new(budget: &QueryBudget) -> Self {
        Self {
            values: Vec::new(),
            memory: budget.memory(0).unwrap(),
            payload: 0,
        }
    }
    pub fn from_vec(values: Vec<T>, budget: &QueryBudget) -> Result<Self, DatabaseError> {
        let mut memory = budget.memory(0)?;
        let payload = if memory.active() {
            values
                .iter()
                .map(Workspace::heap_bytes)
                .fold(0, u64::saturating_add)
        } else {
            0
        };
        memory.resize(
            (values.capacity() as u64)
                .saturating_mul(size_of::<T>() as u64)
                .saturating_add(payload),
        )?;
        Ok(Self {
            values,
            memory,
            payload,
        })
    }
    pub fn push(&mut self, value: T) -> Result<(), DatabaseError> {
        if self.memory.active() {
            let capacity = if self.values.len() == self.values.capacity() {
                self.values.capacity().saturating_mul(2).max(4)
            } else {
                self.values.capacity()
            };
            let payload = self.payload.saturating_add(value.heap_bytes());
            self.memory.resize(
                (capacity as u64)
                    .saturating_mul(size_of::<T>() as u64)
                    .saturating_add(payload),
            )?;
            if capacity > self.values.capacity() {
                self.values
                    .try_reserve_exact(capacity - self.values.len())
                    .map_err(|_| {
                        DatabaseError::Execution("query workspace allocation failed".into())
                    })?;
            }
            self.memory.resize(
                (self.values.capacity() as u64)
                    .saturating_mul(size_of::<T>() as u64)
                    .saturating_add(payload),
            )?;
            self.payload = payload;
        }
        self.values.push(value);
        Ok(())
    }
    pub fn extend(&mut self, values: impl IntoIterator<Item = T>) -> Result<(), DatabaseError> {
        if !self.memory.active() {
            self.values.extend(values);
            return Ok(());
        }
        for value in values {
            self.push(value)?;
        }
        Ok(())
    }
    pub fn into_parts(self) -> (Vec<T>, Reservation) {
        (self.values, self.memory)
    }
    pub fn sort_by(&mut self, compare: impl FnMut(&T, &T) -> std::cmp::Ordering) {
        self.values.sort_by(compare);
    }
}

impl<T: Workspace> Deref for BudgetVec<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.values
    }
}

pub(super) struct BudgetIntoIter<T: Workspace> {
    values: std::vec::IntoIter<T>,
    memory: Reservation,
}

impl<T: Workspace> Iterator for BudgetIntoIter<T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        let value = self.values.next()?;
        if self.memory.active() {
            self.memory.release(value.heap_bytes());
        }
        Some(value)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.values.size_hint()
    }
}

impl<T: Workspace> IntoIterator for BudgetVec<T> {
    type Item = T;
    type IntoIter = BudgetIntoIter<T>;
    fn into_iter(self) -> Self::IntoIter {
        BudgetIntoIter {
            values: self.values.into_iter(),
            memory: self.memory,
        }
    }
}

impl<'a, T: Workspace> IntoIterator for &'a BudgetVec<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.values.iter()
    }
}

/// Deterministic prepaid units for a comparison sort (including key count).
pub(super) fn sort_work(rows: usize, keys: usize) -> u64 {
    if rows < 2 {
        return 0;
    }
    (rows as u64)
        .saturating_mul((usize::BITS - (rows - 1).leading_zeros()) as u64)
        .saturating_mul(keys as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Payload(Vec<u8>);

    impl Workspace for Payload {
        fn heap_bytes(&self) -> u64 {
            self.0.capacity() as u64
        }
    }

    #[test]
    fn retained_capacity_and_payload_release_on_iteration_and_error() {
        let budget = QueryBudget::new(QueryLimits {
            max_memory_bytes: Some(256),
            ..QueryLimits::default()
        });
        let mut rows = BudgetVec::new(&budget);
        rows.push(Payload(vec![0; 16])).unwrap();
        rows.push(Payload(vec![0; 32])).unwrap();
        let retained = budget.0.as_ref().unwrap().memory.get();
        assert!(rows.push(Payload(vec![0; 256])).is_err());
        assert_eq!(budget.0.as_ref().unwrap().memory.get(), retained);
        let mut rows = rows.into_iter();
        drop(rows.next());
        assert_eq!(budget.0.as_ref().unwrap().memory.get(), retained - 16);
        drop(rows);
        assert_eq!(budget.0.as_ref().unwrap().memory.get(), 0);
        budget.memory(256).unwrap();
    }

    #[test]
    fn exact_limits_release_on_drop_and_overflow_fails_closed() {
        let budget = QueryBudget::new(QueryLimits {
            max_work_units: Some(3),
            max_memory_bytes: Some(10),
        });
        budget.work(3).unwrap();
        assert!(budget.work(1).is_err());
        assert!(budget.work(u64::MAX).is_err());
        let mut first = budget.memory(7).unwrap();
        let second = budget.memory(3).unwrap();
        assert!(first.grow(1).is_err());
        assert_eq!(first.bytes(), 7);
        drop(second);
        first.grow(3).unwrap();
        assert!(first.grow(u64::MAX).is_err());
        drop(first);
        assert_eq!(budget.0.as_ref().unwrap().memory.get(), 0);
        assert_eq!(budget.0.as_ref().unwrap().peak.get(), 10);
        budget.memory(10).unwrap();
    }
}
