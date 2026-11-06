//! Linear reduction for the interval order defined by `Time::definitely_before`.
//!
//! A time is dominated iff its interval ends at or before some other interval's
//! start (and the stored times differ). Only the greatest start is needed as a
//! witness: if any interval dominates a candidate, that witness also does. This
//! is NOT the derived `Time::Ord`, and choosing a witness does not choose a single
//! winning row. Every equal or incomparable maximum, including duplicate rows,
//! survives. BOT/EOT use the same identity guard as `definitely_before`.

use super::{TemporalBound, Time};
use crate::construct::OtherHasher;
use crate::error::DatabaseError;
use std::collections::{HashMap, hash_map::Entry};
use std::hash::Hash;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Work {
    Row,
    Comparison,
}

/// A sufficient witness for existence of a strictly later time. The witness's
/// lower bound is cached; all comparisons retain the existing time semantics.
pub(crate) struct Frontier {
    time: Time,
    start: TemporalBound,
}

impl Frontier {
    pub(crate) fn new(time: &Time) -> Self {
        Self {
            time: time.clone(),
            start: time.interval().start,
        }
    }

    /// Returns whether this time replaced the representative witness.
    pub(crate) fn observe(
        &mut self,
        time: &Time,
        work: &mut impl FnMut(Work) -> Result<(), DatabaseError>,
    ) -> Result<bool, DatabaseError> {
        work(Work::Comparison)?;
        let start = time.interval().start;
        if start > self.start {
            self.time = time.clone();
            self.start = start;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub(crate) fn dominates(
        &self,
        candidate: &Time,
        work: &mut impl FnMut(Work) -> Result<(), DatabaseError>,
    ) -> Result<bool, DatabaseError> {
        work(Work::Comparison)?;
        Ok(candidate != &self.time && candidate.interval().end <= self.start)
    }
}

/// Retain a group's temporal maxima in original order, with at most 2(n - 1)
/// ordering comparisons and constant auxiliary memory beyond the output.
pub(crate) fn retain_maximal<T>(
    rows: Vec<T>,
    time: impl Fn(&T) -> &Time,
    mut work: impl FnMut(Work) -> Result<(), DatabaseError>,
) -> Result<Vec<T>, DatabaseError> {
    let Some(first) = rows.first() else {
        return Ok(rows);
    };
    work(Work::Row)?;
    let mut frontier = Frontier::new(time(first));
    let mut witness = 0;
    for (index, row) in rows.iter().enumerate().skip(1) {
        work(Work::Row)?;
        if frontier.observe(time(row), &mut work)? {
            witness = index;
        }
    }
    let mut retained = Vec::new();
    for (index, row) in rows.into_iter().enumerate() {
        work(Work::Row)?;
        if index == witness || !frontier.dominates(time(&row), &mut work)? {
            retained.push(row);
        }
    }
    Ok(retained)
}

/// Group by exact caller-defined identity and retain every group's maxima.
/// Membership slots avoid hashing/cloning keys twice. The two passes preserve
/// input order and bag multiplicity, use O(n + groups) auxiliary memory, and
/// perform exactly 2(n - groups) temporal ordering comparisons.
pub(crate) fn retain_maximal_by_key<T, K: Eq + Hash>(
    rows: Vec<T>,
    key: impl Fn(&T) -> K,
    time: impl Fn(&T) -> &Time,
    mut work: impl FnMut(Work) -> Result<(), DatabaseError>,
) -> Result<Vec<T>, DatabaseError> {
    let mut groups = HashMap::<K, usize, OtherHasher>::default();
    let mut frontiers: Vec<(Frontier, usize)> = Vec::new();
    let mut membership = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        work(Work::Row)?;
        let group = match groups.entry(key(row)) {
            Entry::Vacant(entry) => {
                let group = frontiers.len();
                entry.insert(group);
                frontiers.push((Frontier::new(time(row)), index));
                group
            }
            Entry::Occupied(entry) => {
                let group = *entry.get();
                let (frontier, witness) = &mut frontiers[group];
                if frontier.observe(time(row), &mut work)? {
                    *witness = index;
                }
                group
            }
        };
        membership.push(group);
    }
    drop(groups);
    let mut retained = Vec::new();
    for (index, (row, group)) in rows.into_iter().zip(membership).enumerate() {
        work(Work::Row)?;
        let (frontier, witness) = &frontiers[group];
        if index == *witness || !frontier.dominates(time(&row), &mut work)? {
            retained.push(row);
        }
    }
    Ok(retained)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datatype::TimeType;
    use chrono::NaiveDate;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Row {
        id: usize,
        group: usize,
        time: Time,
    }

    fn times() -> Vec<Time> {
        vec![
            Time::new_beginning_of_time(),
            Time::new_end_of_time(),
            Time::new_year_from("2023").unwrap(),
            Time::new_year_from("2024").unwrap(),
            Time::new_year_month_from("2024-06").unwrap(),
            Time::new_year_month_from("2024-07").unwrap(),
            Time::new_date_from("2024-06-15").unwrap(),
            Time::new_date_from("2024-06-16").unwrap(),
            Time::new_datetime_from("2024-06-15T12:00:00").unwrap(),
            Time::new_datetime_from("2024-06-15T12:00:00.000000001").unwrap(),
            Time::new_datetime_from("2016-12-31T23:59:60").unwrap(),
            Time::new_year_from(&i32::MIN.to_string()).unwrap(),
            Time::new_year_from(&i32::MAX.to_string()).unwrap(),
            Time {
                moment: TimeType::YearMonth(i32::MAX, 12),
            },
            Time {
                moment: TimeType::Date(NaiveDate::MIN),
            },
            Time {
                moment: TimeType::Date(NaiveDate::MAX),
            },
            Time::from_naive_datetime(
                NaiveDate::MAX
                    .and_hms_nano_opt(23, 59, 59, 999_999_999)
                    .unwrap(),
            ),
        ]
    }

    // Intentionally independent, quadratic reference using the public temporal
    // relation. Equality, input order, and duplicate rows are observable here.
    fn oracle(rows: &[Row]) -> Vec<Row> {
        rows.iter()
            .filter(|candidate| {
                !rows.iter().any(|other| {
                    candidate.group == other.group && candidate.time.definitely_before(&other.time)
                })
            })
            .cloned()
            .collect()
    }

    #[test]
    fn every_three_time_combination_matches_the_pairwise_reference() {
        let times = times();
        for a in &times {
            for b in &times {
                for c in &times {
                    let rows = [a, b, c]
                        .into_iter()
                        .enumerate()
                        .map(|(id, time)| Row {
                            id,
                            group: 0,
                            time: time.clone(),
                        })
                        .collect::<Vec<_>>();
                    let expected = oracle(&rows);
                    assert_eq!(
                        retain_maximal(rows.clone(), |row| &row.time, |_| Ok(())).unwrap(),
                        expected
                    );
                    assert_eq!(
                        retain_maximal_by_key(rows, |row| row.group, |row| &row.time, |_| Ok(()))
                            .unwrap(),
                        expected
                    );
                }
            }
        }
    }

    #[test]
    fn grouped_reduction_preserves_bags_order_and_linear_work() {
        let times = times();
        let mut seed = 42_u64;
        for size in [0, 1, 2, 17, 128, 2_048] {
            let mut rows = Vec::new();
            for id in 0..size {
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                rows.push(Row {
                    id,
                    group: (seed >> 32) as usize % 13,
                    time: times[(seed >> 40) as usize % times.len()].clone(),
                });
            }
            let expected = oracle(&rows);
            let mut comparisons = 0;
            let actual = retain_maximal_by_key(
                rows,
                |row| row.group,
                |row| &row.time,
                |work| {
                    comparisons += usize::from(work == Work::Comparison);
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(actual, expected);
            assert!(
                comparisons <= 2 * size,
                "{size} rows took {comparisons} comparisons"
            );
        }
    }

    #[test]
    fn cancellation_and_work_limits_abort_either_pass_without_results() {
        let rows = times()
            .into_iter()
            .enumerate()
            .map(|(id, time)| Row {
                id,
                group: id % 3,
                time,
            })
            .collect::<Vec<_>>();
        let mut visits = 0;
        retain_maximal_by_key(
            rows.clone(),
            |row| row.group,
            |row| &row.time,
            |_| {
                visits += 1;
                Ok(())
            },
        )
        .unwrap();
        for stop in 1..=visits {
            let mut visited = 0;
            let result = retain_maximal_by_key(
                rows.clone(),
                |row| row.group,
                |row| &row.time,
                |_| {
                    visited += 1;
                    if visited == stop {
                        Err(DatabaseError::Cancelled)
                    } else {
                        Ok(())
                    }
                },
            );
            assert!(matches!(result, Err(DatabaseError::Cancelled)));
            assert_eq!(visited, stop);
        }
        let result = retain_maximal(
            rows,
            |row| &row.time,
            |work| {
                if work == Work::Comparison {
                    Err(DatabaseError::ResourceLimit {
                        resource: "test comparisons",
                        limit: 0,
                    })
                } else {
                    Ok(())
                }
            },
        );
        assert!(matches!(result, Err(DatabaseError::ResourceLimit { .. })));
    }
}
