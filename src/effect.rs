//! Source-local information-in-effect selection.
//!
//! This module resolves assertion evidence at independent assertion and target
//! appearance cutoffs. It deliberately does not fuse sources, rank certainty,
//! or select an accepted truth.

use crate::construct::{ASCERTAINS_ROLE_ID, Database, OtherHasher, POSIT_ROLE_ID, Posit, Thing};
use crate::datatype::Time;
use crate::datatype::temporal::{Frontier, Work, retain_maximal_by_key};
use crate::error::DatabaseError;
use crate::literal::LiteralValue;
use roaring::RoaringTreemap;
use std::collections::{HashMap, hash_map::Entry};
use std::sync::Arc;

pub const MAX_EFFECT_ASSERTION_CANDIDATES: u64 = 1_000_000;
pub const MAX_EFFECT_TEMPORAL_COMPARISONS: u64 = 20_000_000;

/// Fail-closed resource limits for one resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectLimits {
    pub assertion_candidates: u64,
    pub temporal_comparisons: u64,
}

impl Default for EffectLimits {
    fn default() -> Self {
        Self {
            assertion_candidates: MAX_EFFECT_ASSERTION_CANDIDATES,
            temporal_comparisons: MAX_EFFECT_TEMPORAL_COMPARISONS,
        }
    }
}

/// The two temporal cuts used to derive an effective assertion slice.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EffectCut {
    assertion_time: Time,
    appearance_time: Time,
}

impl EffectCut {
    pub fn new(assertion_time: Time, appearance_time: Time) -> Self {
        Self {
            assertion_time,
            appearance_time,
        }
    }

    pub fn assertion_time(&self) -> &Time {
        &self.assertion_time
    }

    pub fn appearance_time(&self) -> &Time {
        &self.appearance_time
    }
}

/// Work counters for one information-in-effect resolution.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EffectCounters {
    pub assertion_candidates: u64,
    pub temporally_eligible: u64,
    pub retractions: u64,
    /// Ordering comparisons in retraction lookup and temporal reduction,
    /// including interval-start comparisons used to select dominance witnesses.
    pub temporal_comparisons: u64,
    pub effective_assertions: u64,
}

/// One retained assertion together with its dereferenced target posit.
#[derive(Debug, Clone)]
pub struct EffectiveAssertion {
    assertion: Arc<Posit<LiteralValue>>,
    target: Arc<Posit<LiteralValue>>,
    positor: Thing,
    certainty_percent: i8,
}

impl EffectiveAssertion {
    pub fn assertion_identity(&self) -> Thing {
        self.assertion.posit()
    }

    pub fn target_identity(&self) -> Thing {
        self.target.posit()
    }

    pub fn positor(&self) -> Thing {
        self.positor
    }

    pub fn certainty(&self) -> &LiteralValue {
        self.assertion.value()
    }

    pub fn certainty_percent(&self) -> i8 {
        self.certainty_percent
    }

    pub fn assertion_time(&self) -> &Time {
        self.assertion.time()
    }

    pub fn target(&self) -> Arc<Posit<LiteralValue>> {
        Arc::clone(&self.target)
    }

    pub(crate) fn assertion(&self) -> Arc<Posit<LiteralValue>> {
        Arc::clone(&self.assertion)
    }
}

/// A deterministic source-local evidence slice at one pair of cuts.
#[derive(Debug, Clone)]
pub struct EffectiveSlice {
    cut: EffectCut,
    assertions: Vec<EffectiveAssertion>,
    counters: EffectCounters,
}

impl EffectiveSlice {
    pub fn cut(&self) -> &EffectCut {
        &self.cut
    }

    pub fn assertions(&self) -> &[EffectiveAssertion] {
        &self.assertions
    }

    pub fn counters(&self) -> EffectCounters {
        self.counters
    }
}

impl Database {
    /// Resolve information in effect without applying a truth or fusion policy.
    pub fn information_in_effect(&self, cut: EffectCut) -> Result<EffectiveSlice, DatabaseError> {
        self.information_in_effect_with_limits(cut, EffectLimits::default())
    }

    pub fn information_in_effect_with_limits(
        &self,
        cut: EffectCut,
        limits: EffectLimits,
    ) -> Result<EffectiveSlice, DatabaseError> {
        let _owner = self
            .execution_owner
            .lock()
            .map_err(|error| DatabaseError::Lock(error.to_string()))?;
        resolve(self, cut, limits, || Ok(()))
    }
}

pub(crate) fn resolve(
    database: &Database,
    cut: EffectCut,
    limits: EffectLimits,
    mut check: impl FnMut() -> Result<(), DatabaseError>,
) -> Result<EffectiveSlice, DatabaseError> {
    resolve_budgeted(database, cut, limits, |_| check(), |_| Ok(()))
}

pub(crate) fn resolve_budgeted(
    database: &Database,
    cut: EffectCut,
    limits: EffectLimits,
    mut check: impl FnMut(u64) -> Result<(), DatabaseError>,
    mut reserve: impl FnMut(u64) -> Result<(), DatabaseError>,
) -> Result<EffectiveSlice, DatabaseError> {
    check(1)?;
    let (assertion_identities, mut workspace) =
        assertion_candidates(database, &mut check, &mut reserve)?;
    let mut counters = EffectCounters {
        assertion_candidates: assertion_identities.len(),
        ..EffectCounters::default()
    };
    if counters.assertion_candidates > limits.assertion_candidates {
        return Err(DatabaseError::ResourceLimit {
            resource: "effective assertion candidates",
            limit: limits.assertion_candidates,
        });
    }
    let keeper = database.posit_keeper();
    let mut keeper = keeper
        .lock()
        .map_err(|error| DatabaseError::Lock(error.to_string()))?;
    let mut eligible = Vec::new();

    for identity in assertion_identities {
        check(1)?;
        let assertion = keeper.posit::<LiteralValue>(identity).ok_or_else(|| {
            DatabaseError::Invariant(format!(
                "assertion index contains identity {identity} without a literal posit"
            ))
        })?;
        let appearances = assertion.appearance_set();
        if appearances.appearances().len() != 2 {
            continue;
        }
        let mut target_identity = None;
        let mut positor = None;
        for appearance in appearances.appearances() {
            match appearance.role().role() {
                POSIT_ROLE_ID => target_identity = Some(appearance.thing()),
                ASCERTAINS_ROLE_ID => positor = Some(appearance.thing()),
                _ => {}
            }
        }
        let (Some(target_identity), Some(positor)) = (target_identity, positor) else {
            continue;
        };
        if !assertion
            .time()
            .definitely_at_or_before(cut.assertion_time())
        {
            continue;
        }
        let certainty_percent = assertion
            .value()
            .certainty_percent()
            .map_err(DatabaseError::Invariant)?
            .ok_or_else(|| {
                DatabaseError::Execution(format!(
                    "malformed assertion posit {identity}: exact {{posit, ascertains}} envelopes require a certainty literal"
                ))
            })?;
        let target = keeper.posit::<LiteralValue>(target_identity).ok_or_else(|| {
            DatabaseError::Execution(format!(
                "dangling assertion posit {identity}: target posit {target_identity} does not exist"
            ))
        })?;
        if !target.time().definitely_at_or_before(cut.appearance_time()) {
            continue;
        }
        counters.temporally_eligible += 1;
        if certainty_percent == 0 {
            counters.retractions += 1;
        }
        // Per eligible row: evidence vectors, worst-case group/frontier state,
        // retraction maps, stable-sort scratch, and copied literal group keys.
        workspace = workspace
            .saturating_add(1536)
            .saturating_add((target.value().token().len() as u64).saturating_mul(2));
        reserve(workspace)?;
        eligible.push(EffectiveAssertion {
            assertion,
            target,
            positor,
            certainty_percent,
        });
    }
    drop(keeper);

    let mut work = |work| {
        check(1)?;
        if work == Work::Comparison {
            count_comparison(&mut counters, limits)?;
        }
        Ok(())
    };
    let mut retractions_by_target = HashMap::<(Thing, Thing), Frontier, OtherHasher>::default();
    for candidate in &eligible {
        work(Work::Row)?;
        if candidate.certainty_percent == 0 {
            match retractions_by_target.entry((candidate.positor, candidate.target.posit())) {
                Entry::Vacant(entry) => {
                    entry.insert(Frontier::new(candidate.assertion.time()));
                }
                Entry::Occupied(mut entry) => {
                    entry
                        .get_mut()
                        .observe(candidate.assertion.time(), &mut work)?;
                }
            }
        }
    }

    let mut nonzero = Vec::new();
    for candidate in eligible
        .into_iter()
        .filter(|row| row.certainty_percent != 0)
    {
        work(Work::Row)?;
        if let Some(frontier) =
            retractions_by_target.get(&(candidate.positor, candidate.target.posit()))
            && frontier.dominates(candidate.assertion.time(), &mut work)?
        {
            continue;
        }
        nonzero.push(candidate);
    }
    drop(retractions_by_target);

    let target_maxima = retain_maximal_by_key(
        nonzero,
        |row| (row.positor, row.target.appearance_set()),
        |row| row.target.time(),
        &mut work,
    )?;
    let mut effective = retain_maximal_by_key(
        target_maxima,
        |row| {
            (
                row.positor,
                row.target.appearance_set(),
                row.target.value().clone(),
            )
        },
        |row| row.assertion.time(),
        &mut work,
    )?;
    let rows = effective.len();
    let sort_units = if rows < 2 {
        0
    } else {
        (rows as u64)
            .saturating_mul((usize::BITS - (rows - 1).leading_zeros()) as u64)
            .saturating_mul(7)
    };
    check(sort_units)?;
    effective.sort_by(|left, right| {
        (
            left.positor,
            left.target.appearance_set(),
            left.target.value(),
            left.target.time(),
            left.assertion.time(),
            left.target.posit(),
            left.assertion.posit(),
        )
            .cmp(&(
                right.positor,
                right.target.appearance_set(),
                right.target.value(),
                right.target.time(),
                right.assertion.time(),
                right.target.posit(),
                right.assertion.posit(),
            ))
    });
    check(1)?;
    counters.effective_assertions = effective.len() as u64;
    Ok(EffectiveSlice {
        cut,
        assertions: effective,
        counters,
    })
}

fn assertion_candidates(
    database: &Database,
    check: &mut impl FnMut(u64) -> Result<(), DatabaseError>,
    reserve: &mut impl FnMut(u64) -> Result<(), DatabaseError>,
) -> Result<(RoaringTreemap, u64), DatabaseError> {
    let lookup = database.role_to_posit_thing_lookup();
    let lookup = lookup
        .lock()
        .map_err(|error| DatabaseError::Lock(error.to_string()))?;
    let targets = lookup.lookup(&POSIT_ROLE_ID);
    let sources = lookup.lookup(&ASCERTAINS_ROLE_ID);
    let count = targets
        .map_or(0, RoaringTreemap::len)
        .saturating_add(sources.map_or(0, RoaringTreemap::len));
    check(count.saturating_mul(2))?;
    let workspace = count.saturating_mul(512).saturating_add(256);
    reserve(workspace)?;
    let mut candidates = targets.cloned().unwrap_or_default();
    candidates &= sources.cloned().unwrap_or_default();
    Ok((candidates, workspace))
}

fn count_comparison(
    counters: &mut EffectCounters,
    limits: EffectLimits,
) -> Result<(), DatabaseError> {
    counters.temporal_comparisons = counters
        .temporal_comparisons
        .checked_add(1)
        .ok_or_else(|| DatabaseError::Invariant("effect comparison count overflowed".into()))?;
    if counters.temporal_comparisons > limits.temporal_comparisons {
        return Err(DatabaseError::ResourceLimit {
            resource: "effective temporal comparisons",
            limit: limits.temporal_comparisons,
        });
    }
    Ok(())
}
