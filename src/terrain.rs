//! Authoritative, bounded structural Terrain reports.

use crate::construct::{AppearanceSet, Database, Thing, ThingHasher};
use crate::datatype::{Time, time_is_strictly_dominated};
use crate::error::DatabaseError;
use crate::traqula::CancellationToken;
use roaring::RoaringTreemap;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, MutexGuard, TryLockError};
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(all(target_arch = "wasm32", feature = "wasm"))]
use wasm_bindgen::prelude::wasm_bindgen;

#[cfg(all(target_arch = "wasm32", feature = "wasm"))]
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = performance, js_name = now)]
    fn terrain_performance_now() -> f64;
}

#[cfg(all(target_arch = "wasm32", not(feature = "wasm")))]
fn terrain_performance_now() -> f64 {
    0.0
}

pub const TERRAIN_VERSION: u16 = 1;
pub const DEFAULT_PROJECTED_ROLE_LIMIT: usize = 8;
pub const MAX_PROJECTED_ROLE_LIMIT: usize = 8;
pub const DEFAULT_MAX_RELATIONSHIP_SIGNATURES: usize = 16;
pub const MAX_RELATIONSHIP_SIGNATURES: usize = 128;

pub const MAX_POSITS_SCANNED: u64 = 1_000_000;
pub const MAX_ACTIVE_APPEARANCE_SETS: u64 = 250_000;
pub const MAX_ENDPOINT_THINGS: u64 = 1_000_000;
pub const MAX_TEMPORAL_COMPARISONS: u64 = 5_000_000;
pub const MAX_RELATIONSHIP_ARITY: u64 = 256;
pub const MAX_ALLOCATIONS: u64 = 4_096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerrainLimits {
    pub posits_scanned: u64,
    pub active_appearance_sets: u64,
    pub endpoint_things: u64,
    pub temporal_comparisons: u64,
    pub relationship_arity: u64,
    pub allocations: u64,
}

impl Default for TerrainLimits {
    fn default() -> Self {
        Self {
            posits_scanned: MAX_POSITS_SCANNED,
            active_appearance_sets: MAX_ACTIVE_APPEARANCE_SETS,
            endpoint_things: MAX_ENDPOINT_THINGS,
            temporal_comparisons: MAX_TEMPORAL_COMPARISONS,
            relationship_arity: MAX_RELATIONSHIP_ARITY,
            allocations: MAX_ALLOCATIONS,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TerrainOptions {
    pub as_of: Option<Time>,
    pub timeout: Option<Duration>,
    pub cancellation: Option<CancellationToken>,
    pub projected_role_limit: usize,
    pub max_relationship_signatures: usize,
    pub limits: TerrainLimits,
}

impl Default for TerrainOptions {
    fn default() -> Self {
        Self {
            as_of: None,
            timeout: None,
            cancellation: None,
            projected_role_limit: DEFAULT_PROJECTED_ROLE_LIMIT,
            max_relationship_signatures: DEFAULT_MAX_RELATIONSHIP_SIGNATURES,
            limits: TerrainLimits::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerrainReport {
    pub terrain_version: u16,
    pub resolved_as_of: String,
    pub database: TerrainDatabaseTotals,
    pub projection: TerrainProjection,
    pub relationship_catalog: TerrainRelationshipCatalog,
    pub frames: TerrainFrames,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerrainFrames {
    pub history: TerrainFrame,
    pub current: TerrainFrame,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerrainDatabaseTotals {
    pub referenced_things: u64,
    pub roles: u64,
    pub appearances: u64,
    pub appearance_sets: u64,
    pub posits: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerrainProjection {
    pub complete: bool,
    pub total_attribute_roles: u64,
    pub roles: Vec<TerrainProjectedRole>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerrainProjectedRole {
    pub id: String,
    pub name: String,
    pub bit: u8,
    pub history_support: u64,
    pub current_support: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerrainRelationshipCatalog {
    pub complete: bool,
    pub total_signatures: u64,
    pub default_signature_id: Option<String>,
    pub signatures: Vec<TerrainRelationshipSignature>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerrainRelationshipSignature {
    pub id: String,
    pub roles: Vec<TerrainRoleRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerrainRoleRef {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerrainFrame {
    pub scope: TerrainScope,
    pub stats: TerrainFrameStats,
    pub role_supports: Vec<TerrainRoleSupport>,
    pub profiles: Vec<TerrainProfile>,
    pub isopleths: Vec<TerrainIsopleth>,
    pub relationships: Vec<TerrainRelationship>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerrainScope {
    History,
    Current,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerrainFrameStats {
    pub endpoint_things: u64,
    pub roles: u64,
    pub appearance_sets: u64,
    pub posits: u64,
    pub incidences: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerrainRoleSupport {
    pub role_id: String,
    pub distinct_things: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerrainProfile {
    pub id: String,
    pub mask: u16,
    pub present_role_ids: Vec<String>,
    pub absent_role_ids: Vec<String>,
    pub things: u64,
    pub isopleth_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerrainIsopleth {
    pub id: String,
    pub mask: u16,
    pub included_role_ids: Vec<String>,
    pub support: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerrainRelationship {
    pub signature_id: String,
    pub appearance_sets: u64,
    pub posits: u64,
    pub role_totals: Vec<TerrainRoleTotal>,
    pub allocations: Vec<TerrainAllocation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerrainRoleTotal {
    pub role_id: String,
    pub distinct_things: u64,
    pub participations: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerrainAllocation {
    pub id: String,
    pub role_id: String,
    pub profile_id: String,
    pub profile_mask: u16,
    pub isopleth_id: Option<String>,
    pub distinct_things: u64,
    pub participations: u64,
}

#[derive(Clone)]
struct CapturedPosit {
    posit: Thing,
    appearance_set: Arc<AppearanceSet>,
    time: Time,
}

struct CapturedDatabase {
    totals: TerrainDatabaseTotals,
    role_names: BTreeMap<Thing, String>,
    posits: Vec<CapturedPosit>,
}

struct TerrainExecution {
    #[cfg(not(target_arch = "wasm32"))]
    deadline: Option<Instant>,
    #[cfg(target_arch = "wasm32")]
    deadline_ms: Option<f64>,
    cancellation: CancellationToken,
    limits: TerrainLimits,
    temporal_comparisons: u64,
}

impl TerrainExecution {
    fn new(options: &TerrainOptions) -> Result<Self, DatabaseError> {
        validate_limit(
            "posits scanned",
            options.limits.posits_scanned,
            MAX_POSITS_SCANNED,
        )?;
        validate_limit(
            "active appearance sets",
            options.limits.active_appearance_sets,
            MAX_ACTIVE_APPEARANCE_SETS,
        )?;
        validate_limit(
            "endpoint Things",
            options.limits.endpoint_things,
            MAX_ENDPOINT_THINGS,
        )?;
        validate_limit(
            "temporal comparisons",
            options.limits.temporal_comparisons,
            MAX_TEMPORAL_COMPARISONS,
        )?;
        validate_limit(
            "relationship arity",
            options.limits.relationship_arity,
            MAX_RELATIONSHIP_ARITY,
        )?;
        validate_limit("allocations", options.limits.allocations, MAX_ALLOCATIONS)?;
        if options.projected_role_limit > MAX_PROJECTED_ROLE_LIMIT {
            return Err(resource_limit(
                "projected attribute Roles",
                MAX_PROJECTED_ROLE_LIMIT as u64,
            ));
        }
        if options.max_relationship_signatures > MAX_RELATIONSHIP_SIGNATURES {
            return Err(resource_limit(
                "relationship signatures",
                MAX_RELATIONSHIP_SIGNATURES as u64,
            ));
        }
        #[cfg(not(target_arch = "wasm32"))]
        let deadline = options
            .timeout
            .map(|timeout| {
                Instant::now()
                    .checked_add(timeout)
                    .ok_or_else(|| DatabaseError::Execution("Terrain timeout is too large".into()))
            })
            .transpose()?;
        #[cfg(target_arch = "wasm32")]
        let deadline_ms = options
            .timeout
            .map(|timeout| {
                let deadline = terrain_performance_now() + timeout.as_secs_f64() * 1_000.0;
                deadline
                    .is_finite()
                    .then_some(deadline)
                    .ok_or_else(|| DatabaseError::Execution("Terrain timeout is too large".into()))
            })
            .transpose()?;
        Ok(Self {
            #[cfg(not(target_arch = "wasm32"))]
            deadline,
            #[cfg(target_arch = "wasm32")]
            deadline_ms,
            cancellation: options.cancellation.clone().unwrap_or_default(),
            limits: options.limits.clone(),
            temporal_comparisons: 0,
        })
    }

    fn check(&self) -> Result<(), DatabaseError> {
        if self.cancellation.is_cancelled() {
            return Err(DatabaseError::Cancelled);
        }
        #[cfg(not(target_arch = "wasm32"))]
        let expired = self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline);
        #[cfg(target_arch = "wasm32")]
        let expired = self
            .deadline_ms
            .is_some_and(|deadline| terrain_performance_now() >= deadline);
        if expired {
            return Err(DatabaseError::Timeout);
        }
        Ok(())
    }

    fn checkpoint(&self, index: usize) -> Result<(), DatabaseError> {
        if index.is_multiple_of(1_024) {
            self.check()?;
        }
        Ok(())
    }

    fn temporal_comparison(&mut self) -> Result<(), DatabaseError> {
        self.temporal_comparisons =
            checked_add(self.temporal_comparisons, 1, "temporal comparisons")?;
        if self.temporal_comparisons > self.limits.temporal_comparisons {
            return Err(resource_limit(
                "temporal comparisons",
                self.limits.temporal_comparisons,
            ));
        }
        if self.temporal_comparisons.is_multiple_of(1_024) {
            self.check()?;
        }
        Ok(())
    }
}

fn validate_limit(resource: &'static str, value: u64, maximum: u64) -> Result<(), DatabaseError> {
    if value > maximum {
        Err(resource_limit(resource, maximum))
    } else {
        Ok(())
    }
}

fn resource_limit(resource: &'static str, limit: u64) -> DatabaseError {
    DatabaseError::ResourceLimit { resource, limit }
}

fn checked_add(left: u64, right: u64, resource: &'static str) -> Result<u64, DatabaseError> {
    left.checked_add(right)
        .ok_or_else(|| resource_limit(resource, u64::MAX))
}

fn checked_mul(left: u64, right: u64, resource: &'static str) -> Result<u64, DatabaseError> {
    left.checked_mul(right)
        .ok_or_else(|| resource_limit(resource, u64::MAX))
}

fn usize_count(value: usize, resource: &'static str) -> Result<u64, DatabaseError> {
    u64::try_from(value).map_err(|_| resource_limit(resource, u64::MAX))
}

fn role_id(role: Thing) -> String {
    role.to_string()
}

fn profile_id(mask: u16) -> String {
    format!("terrain-v{TERRAIN_VERSION}-profile-{mask:03x}")
}

fn isopleth_id(mask: u16) -> String {
    format!("terrain-v{TERRAIN_VERSION}-isopleth-{mask:03x}")
}

fn signature_id(signature: &[Thing]) -> String {
    let roles = signature
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("-");
    format!("terrain-v{TERRAIN_VERSION}-signature-{roles}")
}

fn allocation_id(signature: &[Thing], role: Thing, mask: u16) -> String {
    format!(
        "terrain-v{TERRAIN_VERSION}-allocation-{}-{role}-{mask:03x}",
        signature
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("-")
    )
}

/// A canonical Traqula token suitable for reusing the exact resolved cutoff.
pub fn terrain_time_token(time: &Time) -> String {
    match time.to_string().as_str() {
        "BOT" => "@BOT".to_string(),
        "EOT" => "@EOT".to_string(),
        value => format!("'{value}'"),
    }
}

impl Database {
    pub fn terrain(&self) -> Result<TerrainReport, DatabaseError> {
        self.terrain_with_options(TerrainOptions::default())
    }

    pub fn terrain_with_options(
        &self,
        options: TerrainOptions,
    ) -> Result<TerrainReport, DatabaseError> {
        let mut execution = TerrainExecution::new(&options)?;
        let cutoff = options.as_of.clone().unwrap_or_default();
        execution.check()?;
        let owner = acquire_owner(self, &execution)?;
        let captured = self.capture_terrain(&mut execution)?;
        drop(owner);
        execution.check()?;
        aggregate(
            captured,
            cutoff,
            options.projected_role_limit,
            options.max_relationship_signatures,
            &mut execution,
        )
    }

    fn capture_terrain(
        &self,
        execution: &mut TerrainExecution,
    ) -> Result<CapturedDatabase, DatabaseError> {
        // Lock order is fixed for Terrain capture: Thing generator, Role keeper,
        // Appearance keeper, AppearanceSet keeper, Posit keeper, reverse set map,
        // then reverse time map. The execution owner excludes structural mutation.
        let referenced_things = usize_count(
            self.thing_generator
                .lock()
                .map_err(|error| DatabaseError::Lock(error.to_string()))?
                .len(),
            "referenced Things",
        )?;
        execution.check()?;
        let roles = self
            .role_keeper
            .lock()
            .map_err(|error| DatabaseError::Lock(error.to_string()))?
            .snapshot();
        let role_count = usize_count(roles.len(), "Roles")?;
        execution.check()?;
        let appearances = usize_count(
            self.appearance_keeper
                .lock()
                .map_err(|error| DatabaseError::Lock(error.to_string()))?
                .len(),
            "Appearances",
        )?;
        execution.check()?;
        let appearance_sets = usize_count(
            self.appearance_set_keeper
                .lock()
                .map_err(|error| DatabaseError::Lock(error.to_string()))?
                .len(),
            "AppearanceSets",
        )?;
        execution.check()?;
        let posit_count = usize_count(
            self.posit_keeper
                .lock()
                .map_err(|error| DatabaseError::Lock(error.to_string()))?
                .len(),
            "Posits",
        )?;
        if posit_count > execution.limits.posits_scanned {
            return Err(resource_limit(
                "posits scanned",
                execution.limits.posits_scanned,
            ));
        }
        execution.check()?;

        let appearance_sets_by_posit = self
            .posit_thing_to_appearance_set_lookup
            .lock()
            .map_err(|error| DatabaseError::Lock(error.to_string()))?;
        let times_by_posit = self
            .posit_time_lookup
            .lock()
            .map_err(|error| DatabaseError::Lock(error.to_string()))?;
        if appearance_sets_by_posit.len() != times_by_posit.len()
            || appearance_sets_by_posit.len() != usize::try_from(posit_count).unwrap_or(usize::MAX)
            || appearance_sets_by_posit
                .keys()
                .any(|posit| !times_by_posit.contains_key(posit))
        {
            return Err(DatabaseError::Invariant(
                "Terrain capture found mismatched posit reverse indexes".to_string(),
            ));
        }
        let mut posits = Vec::with_capacity(appearance_sets_by_posit.len());
        for (index, (posit, appearance_set)) in appearance_sets_by_posit.iter().enumerate() {
            execution.checkpoint(index)?;
            let time = times_by_posit.get(posit).ok_or_else(|| {
                DatabaseError::Invariant(format!("Terrain capture has no time for posit {posit}"))
            })?;
            posits.push(CapturedPosit {
                posit: *posit,
                appearance_set: Arc::clone(appearance_set),
                time: time.clone(),
            });
        }
        posits.sort_by_key(|posit| posit.posit);
        execution.check()?;

        let role_names = roles
            .into_iter()
            .map(|role| (role.role(), role.name().to_string()))
            .collect();
        Ok(CapturedDatabase {
            totals: TerrainDatabaseTotals {
                referenced_things,
                roles: role_count,
                appearances,
                appearance_sets,
                posits: posit_count,
            },
            role_names,
            posits,
        })
    }
}

fn acquire_owner<'a>(
    database: &'a Database,
    execution: &TerrainExecution,
) -> Result<MutexGuard<'a, ()>, DatabaseError> {
    loop {
        match database.execution_owner.try_lock() {
            Ok(owner) => return Ok(owner),
            Err(TryLockError::Poisoned(error)) => {
                return Err(DatabaseError::Lock(error.to_string()));
            }
            Err(TryLockError::WouldBlock) => {
                execution.check()?;
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
}

#[derive(Default)]
struct SignatureWork {
    posits: u64,
    sets: Vec<Arc<AppearanceSet>>,
}

struct FrameWork {
    stats: TerrainFrameStats,
    endpoint_things: RoaringTreemap,
    attribute_things: BTreeMap<Thing, RoaringTreemap>,
    signatures: BTreeMap<Vec<Thing>, SignatureWork>,
    profile_masks: HashMap<Thing, u16, ThingHasher>,
}

fn aggregate(
    captured: CapturedDatabase,
    cutoff: Time,
    projected_role_limit: usize,
    max_relationship_signatures: usize,
    execution: &mut TerrainExecution,
) -> Result<TerrainReport, DatabaseError> {
    let mut groups: BTreeMap<Arc<AppearanceSet>, Vec<CapturedPosit>> = BTreeMap::new();
    for (index, posit) in captured.posits.iter().enumerate() {
        execution.checkpoint(index)?;
        groups
            .entry(Arc::clone(&posit.appearance_set))
            .or_default()
            .push(posit.clone());
    }
    check_count_limit(
        usize_count(groups.len(), "active appearance sets")?,
        execution.limits.active_appearance_sets,
        "active appearance sets",
    )?;

    let history_records = captured.posits.clone();
    let mut current_records = Vec::new();
    for records in groups.values() {
        execution.check()?;
        let mut eligible = Vec::new();
        for record in records {
            execution.temporal_comparison()?;
            if record.time.definitely_at_or_before(&cutoff) {
                eligible.push(record);
            }
        }
        for (candidate_index, candidate) in eligible.iter().enumerate() {
            let mut dominated = false;
            for (other_index, other) in eligible.iter().enumerate() {
                if candidate_index == other_index {
                    continue;
                }
                execution.temporal_comparison()?;
                if time_is_strictly_dominated(&candidate.time, &other.time) {
                    dominated = true;
                    break;
                }
            }
            if !dominated {
                current_records.push((*candidate).clone());
            }
        }
    }
    current_records.sort_by_key(|record| record.posit);

    let mut history = build_frame_work(&history_records, execution)?;
    let mut current = build_frame_work(&current_records, execution)?;
    let projection = build_projection(
        &history,
        &current,
        &captured.role_names,
        projected_role_limit,
    )?;
    assign_profiles(&mut history, &projection, execution)?;
    assign_profiles(&mut current, &projection, execution)?;
    let (relationship_catalog, selected_signatures) = build_catalog(
        &history,
        &current,
        &captured.role_names,
        max_relationship_signatures,
    )?;

    let mut allocation_count = 0;
    let history_frame = finish_frame(
        TerrainScope::History,
        &history,
        &projection,
        &selected_signatures,
        execution,
        &mut allocation_count,
    )?;
    let current_frame = finish_frame(
        TerrainScope::Current,
        &current,
        &projection,
        &selected_signatures,
        execution,
        &mut allocation_count,
    )?;
    execution.check()?;
    Ok(TerrainReport {
        terrain_version: TERRAIN_VERSION,
        resolved_as_of: terrain_time_token(&cutoff),
        database: captured.totals,
        projection,
        relationship_catalog,
        frames: TerrainFrames {
            history: history_frame,
            current: current_frame,
        },
    })
}

fn check_count_limit(count: u64, limit: u64, resource: &'static str) -> Result<(), DatabaseError> {
    if count > limit {
        Err(resource_limit(resource, limit))
    } else {
        Ok(())
    }
}

fn build_frame_work(
    records: &[CapturedPosit],
    execution: &TerrainExecution,
) -> Result<FrameWork, DatabaseError> {
    let mut active_sets: BTreeMap<Arc<AppearanceSet>, u64> = BTreeMap::new();
    for (index, record) in records.iter().enumerate() {
        execution.checkpoint(index)?;
        let count = active_sets
            .entry(Arc::clone(&record.appearance_set))
            .or_default();
        *count = checked_add(*count, 1, "frame posits")?;
    }
    check_count_limit(
        usize_count(active_sets.len(), "active appearance sets")?,
        execution.limits.active_appearance_sets,
        "active appearance sets",
    )?;

    let mut endpoint_things = RoaringTreemap::new();
    let mut active_roles = RoaringTreemap::new();
    let mut attribute_things: BTreeMap<Thing, RoaringTreemap> = BTreeMap::new();
    let mut signatures: BTreeMap<Vec<Thing>, SignatureWork> = BTreeMap::new();
    let mut incidences = 0;
    for (index, (appearance_set, posit_count)) in active_sets.iter().enumerate() {
        execution.checkpoint(index)?;
        let appearances = appearance_set.appearances();
        let arity = usize_count(appearances.len(), "relationship arity")?;
        if appearances.len() >= 2 && arity > execution.limits.relationship_arity {
            return Err(resource_limit(
                "relationship arity",
                execution.limits.relationship_arity,
            ));
        }
        incidences = checked_add(
            incidences,
            checked_mul(arity, *posit_count, "frame incidences")?,
            "frame incidences",
        )?;
        for appearance in appearances {
            endpoint_things.insert(appearance.thing());
            active_roles.insert(appearance.role().role());
        }
        check_count_limit(
            endpoint_things.len(),
            execution.limits.endpoint_things,
            "endpoint Things",
        )?;
        if appearances.len() == 1 {
            let appearance = &appearances[0];
            attribute_things
                .entry(appearance.role().role())
                .or_default()
                .insert(appearance.thing());
        } else if appearances.len() >= 2 {
            let signature = appearances
                .iter()
                .map(|appearance| appearance.role().role())
                .collect::<Vec<_>>();
            let work = signatures.entry(signature).or_default();
            work.posits = checked_add(work.posits, *posit_count, "relationship posits")?;
            work.sets.push(Arc::clone(appearance_set));
        }
    }
    Ok(FrameWork {
        stats: TerrainFrameStats {
            endpoint_things: endpoint_things.len(),
            roles: active_roles.len(),
            appearance_sets: usize_count(active_sets.len(), "active appearance sets")?,
            posits: usize_count(records.len(), "frame posits")?,
            incidences,
        },
        endpoint_things,
        attribute_things,
        signatures,
        profile_masks: HashMap::default(),
    })
}

fn support(frame: &FrameWork, role: Thing) -> u64 {
    frame
        .attribute_things
        .get(&role)
        .map_or(0, RoaringTreemap::len)
}

fn role_name(role_names: &BTreeMap<Thing, String>, role: Thing) -> Result<&str, DatabaseError> {
    role_names
        .get(&role)
        .map(String::as_str)
        .ok_or_else(|| DatabaseError::Invariant(format!("Terrain references unknown Role {role}")))
}

fn build_projection(
    history: &FrameWork,
    current: &FrameWork,
    role_names: &BTreeMap<Thing, String>,
    limit: usize,
) -> Result<TerrainProjection, DatabaseError> {
    let mut candidates = history
        .attribute_things
        .keys()
        .chain(current.attribute_things.keys())
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        support(current, *right)
            .cmp(&support(current, *left))
            .then_with(|| support(history, *right).cmp(&support(history, *left)))
            .then_with(|| role_names.get(left).cmp(&role_names.get(right)))
            .then_with(|| left.cmp(right))
    });
    let total_attribute_roles = usize_count(candidates.len(), "attribute Roles")?;
    let complete = candidates.len() <= limit;
    let roles = candidates
        .into_iter()
        .take(limit)
        .enumerate()
        .map(|(bit, role)| {
            Ok(TerrainProjectedRole {
                id: role_id(role),
                name: role_name(role_names, role)?.to_string(),
                bit: u8::try_from(bit).map_err(|_| {
                    resource_limit("projected attribute Roles", MAX_PROJECTED_ROLE_LIMIT as u64)
                })?,
                history_support: support(history, role),
                current_support: support(current, role),
            })
        })
        .collect::<Result<Vec<_>, DatabaseError>>()?;
    Ok(TerrainProjection {
        complete,
        total_attribute_roles,
        roles,
    })
}

fn assign_profiles(
    frame: &mut FrameWork,
    projection: &TerrainProjection,
    execution: &TerrainExecution,
) -> Result<(), DatabaseError> {
    for (index, thing) in frame.endpoint_things.iter().enumerate() {
        execution.checkpoint(index)?;
        let mut mask = 0u16;
        for role in &projection.roles {
            let identity = role.id.parse::<Thing>().map_err(|_| {
                DatabaseError::Invariant(format!("invalid projected Role id {}", role.id))
            })?;
            if frame
                .attribute_things
                .get(&identity)
                .is_some_and(|things| things.contains(thing))
            {
                mask |= 1u16 << role.bit;
            }
        }
        frame.profile_masks.insert(thing, mask);
    }
    Ok(())
}

fn build_catalog(
    history: &FrameWork,
    current: &FrameWork,
    role_names: &BTreeMap<Thing, String>,
    limit: usize,
) -> Result<(TerrainRelationshipCatalog, Vec<Vec<Thing>>), DatabaseError> {
    let mut signatures = history
        .signatures
        .keys()
        .chain(current.signatures.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let set_count = |frame: &FrameWork, signature: &[Thing]| {
        frame
            .signatures
            .get(signature)
            .map_or(0usize, |work| work.sets.len())
    };
    signatures.sort_by(|left, right| {
        set_count(current, right)
            .cmp(&set_count(current, left))
            .then_with(|| set_count(history, right).cmp(&set_count(history, left)))
            .then_with(|| right.len().cmp(&left.len()))
            .then_with(|| {
                let left_names = left
                    .iter()
                    .map(|role| role_names.get(role))
                    .collect::<Vec<_>>();
                let right_names = right
                    .iter()
                    .map(|role| role_names.get(role))
                    .collect::<Vec<_>>();
                left_names.cmp(&right_names)
            })
            .then_with(|| left.cmp(right))
    });
    let total_signatures = usize_count(signatures.len(), "relationship signatures")?;
    let complete = signatures.len() <= limit;
    let selected = signatures.into_iter().take(limit).collect::<Vec<_>>();
    let returned = selected
        .iter()
        .map(|signature| {
            Ok(TerrainRelationshipSignature {
                id: signature_id(signature),
                roles: signature
                    .iter()
                    .map(|role| {
                        Ok(TerrainRoleRef {
                            id: role_id(*role),
                            name: role_name(role_names, *role)?.to_string(),
                        })
                    })
                    .collect::<Result<Vec<_>, DatabaseError>>()?,
            })
        })
        .collect::<Result<Vec<_>, DatabaseError>>()?;
    Ok((
        TerrainRelationshipCatalog {
            complete,
            total_signatures,
            default_signature_id: returned.first().map(|signature| signature.id.clone()),
            signatures: returned,
        },
        selected,
    ))
}

fn finish_frame(
    scope: TerrainScope,
    frame: &FrameWork,
    projection: &TerrainProjection,
    signatures: &[Vec<Thing>],
    execution: &TerrainExecution,
    allocation_count: &mut u64,
) -> Result<TerrainFrame, DatabaseError> {
    let role_supports = projection
        .roles
        .iter()
        .map(|role| {
            let identity = role.id.parse::<Thing>().map_err(|_| {
                DatabaseError::Invariant(format!("invalid projected Role id {}", role.id))
            })?;
            Ok(TerrainRoleSupport {
                role_id: role.id.clone(),
                distinct_things: support(frame, identity),
            })
        })
        .collect::<Result<Vec<_>, DatabaseError>>()?;

    let population_len = 1usize << projection.roles.len();
    let mut populations = vec![0u64; population_len];
    for mask in frame.profile_masks.values() {
        let population = populations.get_mut(usize::from(*mask)).ok_or_else(|| {
            DatabaseError::Invariant(format!("profile mask {mask} is outside projection"))
        })?;
        *population = checked_add(*population, 1, "profile population")?;
    }
    let mut support_sums = populations.clone();
    for bit in 0..projection.roles.len() {
        for mask in 0..population_len {
            if mask & (1usize << bit) == 0 {
                support_sums[mask] = checked_add(
                    support_sums[mask],
                    support_sums[mask | (1usize << bit)],
                    "isopleth support",
                )?;
            }
        }
    }
    let mut profiles = Vec::new();
    let mut isopleths = Vec::new();
    for (mask, population) in populations.iter().copied().enumerate() {
        if population == 0 {
            continue;
        }
        let mask =
            u16::try_from(mask).map_err(|_| resource_limit("profile mask", u16::MAX as u64))?;
        let present_role_ids = projection
            .roles
            .iter()
            .filter(|role| mask & (1u16 << role.bit) != 0)
            .map(|role| role.id.clone())
            .collect::<Vec<_>>();
        let absent_role_ids = projection
            .roles
            .iter()
            .filter(|role| mask & (1u16 << role.bit) == 0)
            .map(|role| role.id.clone())
            .collect::<Vec<_>>();
        profiles.push(TerrainProfile {
            id: profile_id(mask),
            mask,
            present_role_ids: present_role_ids.clone(),
            absent_role_ids,
            things: population,
            isopleth_id: (mask != 0).then(|| isopleth_id(mask)),
        });
        if mask != 0 {
            isopleths.push(TerrainIsopleth {
                id: isopleth_id(mask),
                mask,
                included_role_ids: present_role_ids,
                support: support_sums[usize::from(mask)],
            });
        }
    }
    let population_total = profiles.iter().try_fold(0u64, |total, profile| {
        checked_add(total, profile.things, "profile population")
    })?;
    if population_total != frame.stats.endpoint_things {
        return Err(DatabaseError::Invariant(format!(
            "profile population {population_total} does not equal endpoint Things {}",
            frame.stats.endpoint_things
        )));
    }
    isopleths.sort_by(|left, right| {
        left.mask
            .count_ones()
            .cmp(&right.mask.count_ones())
            .then_with(|| right.support.cmp(&left.support))
            .then_with(|| left.mask.cmp(&right.mask))
    });
    let relationships = build_relationships(frame, signatures, execution, allocation_count)?;
    Ok(TerrainFrame {
        scope,
        stats: frame.stats.clone(),
        role_supports,
        profiles,
        isopleths,
        relationships,
    })
}

#[derive(Default)]
struct RoleCounts {
    things: RoaringTreemap,
    participations: u64,
}

#[derive(Default)]
struct AllocationCounts {
    things: RoaringTreemap,
    participations: u64,
}

fn build_relationships(
    frame: &FrameWork,
    signatures: &[Vec<Thing>],
    execution: &TerrainExecution,
    allocation_count: &mut u64,
) -> Result<Vec<TerrainRelationship>, DatabaseError> {
    let mut relationships = Vec::with_capacity(signatures.len());
    for (signature_index, signature) in signatures.iter().enumerate() {
        execution.checkpoint(signature_index)?;
        let mut role_counts = signature
            .iter()
            .copied()
            .map(|role| (role, RoleCounts::default()))
            .collect::<BTreeMap<_, _>>();
        let mut allocations: BTreeMap<(Thing, u16), AllocationCounts> = BTreeMap::new();
        let work = frame.signatures.get(signature);
        if let Some(work) = work {
            for (set_index, appearance_set) in work.sets.iter().enumerate() {
                execution.checkpoint(set_index)?;
                for appearance in appearance_set.appearances() {
                    let role = appearance.role().role();
                    let counts = role_counts.get_mut(&role).ok_or_else(|| {
                        DatabaseError::Invariant(format!(
                            "relationship signature omits Role {role}"
                        ))
                    })?;
                    counts.things.insert(appearance.thing());
                    counts.participations =
                        checked_add(counts.participations, 1, "relationship participations")?;
                    let mask = *frame
                        .profile_masks
                        .get(&appearance.thing())
                        .ok_or_else(|| {
                            DatabaseError::Invariant(format!(
                                "relationship endpoint {} has no profile",
                                appearance.thing()
                            ))
                        })?;
                    let allocation = allocations.entry((role, mask)).or_default();
                    allocation.things.insert(appearance.thing());
                    allocation.participations =
                        checked_add(allocation.participations, 1, "allocation participations")?;
                }
            }
        }
        *allocation_count = checked_add(
            *allocation_count,
            usize_count(allocations.len(), "allocations")?,
            "allocations",
        )?;
        check_count_limit(
            *allocation_count,
            execution.limits.allocations,
            "allocations",
        )?;
        let role_totals = role_counts
            .iter()
            .map(|(role, counts)| TerrainRoleTotal {
                role_id: role_id(*role),
                distinct_things: counts.things.len(),
                participations: counts.participations,
            })
            .collect::<Vec<_>>();
        let allocation_dtos = allocations
            .iter()
            .map(|((role, mask), counts)| TerrainAllocation {
                id: allocation_id(signature, *role, *mask),
                role_id: role_id(*role),
                profile_id: profile_id(*mask),
                profile_mask: *mask,
                isopleth_id: (*mask != 0).then(|| isopleth_id(*mask)),
                distinct_things: counts.things.len(),
                participations: counts.participations,
            })
            .collect::<Vec<_>>();
        for total in &role_totals {
            let (distinct, participations) = allocation_dtos
                .iter()
                .filter(|allocation| allocation.role_id == total.role_id)
                .try_fold((0u64, 0u64), |(distinct, participations), allocation| {
                    Ok::<_, DatabaseError>((
                        checked_add(
                            distinct,
                            allocation.distinct_things,
                            "allocation distinct Things",
                        )?,
                        checked_add(
                            participations,
                            allocation.participations,
                            "allocation participations",
                        )?,
                    ))
                })?;
            if distinct != total.distinct_things || participations != total.participations {
                return Err(DatabaseError::Invariant(format!(
                    "allocations do not reconcile for relationship Role {}",
                    total.role_id
                )));
            }
        }
        let appearance_sets = work
            .map(|work| usize_count(work.sets.len(), "relationship appearance sets"))
            .transpose()?
            .unwrap_or(0);
        relationships.push(TerrainRelationship {
            signature_id: signature_id(signature),
            appearance_sets,
            posits: work.map_or(0, |work| work.posits),
            role_totals,
            allocations: allocation_dtos,
        });
    }
    Ok(relationships)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Engine, PersistenceMode};
    use std::sync::Arc;

    fn fixed_time(value: &str) -> Time {
        Time::new_date_from(value).unwrap()
    }

    #[test]
    fn empty_database_report_includes_builtins_only_in_database_totals() {
        let database = Database::new(PersistenceMode::InMemory).unwrap();
        let report = database
            .terrain_with_options(TerrainOptions {
                as_of: Some(fixed_time("2025-01-01")),
                ..TerrainOptions::default()
            })
            .unwrap();
        assert_eq!(report.database.referenced_things, 5);
        assert_eq!(report.database.roles, 5);
        assert_eq!(report.database.appearances, 0);
        assert_eq!(report.database.appearance_sets, 0);
        assert_eq!(report.database.posits, 0);
        assert_eq!(report.frames.history.stats.roles, 0);
        assert_eq!(report.frames.current.stats.roles, 0);
        assert!(report.projection.roles.is_empty());
        assert!(report.relationship_catalog.signatures.is_empty());
    }

    #[test]
    fn future_and_repeated_history_follow_snapshot_semantics() {
        let database = Database::new(PersistenceMode::InMemory).unwrap();
        Engine::new(&database)
            .execute(
                "add role name; \
                 add posit [{(+person, name)}, \"old\", '2024-01-01'], \
                           [{(person, name)}, \"new\", '2025-01-01'], \
                           [{(person, name)}, \"future\", '2030-01-01'];",
            )
            .unwrap();
        let report = database
            .terrain_with_options(TerrainOptions {
                as_of: Some(fixed_time("2025-01-01")),
                ..TerrainOptions::default()
            })
            .unwrap();
        assert_eq!(report.frames.history.stats.posits, 3);
        assert_eq!(report.frames.history.stats.incidences, 3);
        assert_eq!(report.frames.current.stats.posits, 1);
        assert_eq!(report.frames.current.stats.incidences, 1);
        assert_eq!(report.projection.roles[0].history_support, 1);
        assert_eq!(report.projection.roles[0].current_support, 1);
    }

    #[test]
    fn projection_limit_is_bounded_and_explicit() {
        let database = Database::new(PersistenceMode::InMemory).unwrap();
        Engine::new(&database)
            .execute(
                "add role a, b; \
                 add posit [{(+x, a)}, 1, '2024-01-01'], [{(x, b)}, 2, '2024-01-01'];",
            )
            .unwrap();
        let report = database
            .terrain_with_options(TerrainOptions {
                as_of: Some(fixed_time("2025-01-01")),
                projected_role_limit: 1,
                ..TerrainOptions::default()
            })
            .unwrap();
        assert!(!report.projection.complete);
        assert_eq!(report.projection.total_attribute_roles, 2);
        assert_eq!(report.projection.roles.len(), 1);
        assert_eq!(
            report
                .frames
                .history
                .profiles
                .iter()
                .map(|p| p.things)
                .sum::<u64>(),
            1
        );
    }

    #[test]
    fn configured_hard_limits_fail_without_a_report() {
        let database = Database::new(PersistenceMode::InMemory).unwrap();
        Engine::new(&database)
            .execute("add role a; add posit [{(+x, a)}, 1, '2024-01-01'];")
            .unwrap();
        let error = database
            .terrain_with_options(TerrainOptions {
                limits: TerrainLimits {
                    posits_scanned: 0,
                    ..TerrainLimits::default()
                },
                ..TerrainOptions::default()
            })
            .unwrap_err();
        assert!(matches!(
            error,
            DatabaseError::ResourceLimit {
                resource: "posits scanned",
                limit: 0
            }
        ));
    }

    #[test]
    fn timeout_and_cancellation_are_observed_while_waiting_for_owner() {
        let database = Arc::new(Database::new(PersistenceMode::InMemory).unwrap());
        let owner = database.execution_owner.lock().unwrap();
        let waiting = Arc::clone(&database);
        let worker = std::thread::spawn(move || {
            waiting.terrain_with_options(TerrainOptions {
                timeout: Some(Duration::from_millis(3)),
                ..TerrainOptions::default()
            })
        });
        assert!(matches!(
            worker.join().unwrap(),
            Err(DatabaseError::Timeout)
        ));

        let cancellation = CancellationToken::new();
        let waiting = Arc::clone(&database);
        let worker_cancellation = cancellation.clone();
        let worker = std::thread::spawn(move || {
            waiting.terrain_with_options(TerrainOptions {
                cancellation: Some(worker_cancellation),
                ..TerrainOptions::default()
            })
        });
        cancellation.cancel();
        assert!(matches!(
            worker.join().unwrap(),
            Err(DatabaseError::Cancelled)
        ));
        drop(owner);
    }

    #[test]
    fn structural_index_mismatch_fails_closed() {
        let database = Database::new(PersistenceMode::InMemory).unwrap();
        Engine::new(&database)
            .execute("add role a; add posit [{(+x, a)}, 1, '2024-01-01'];")
            .unwrap();
        let posit = *database
            .posit_time_lookup
            .lock()
            .unwrap()
            .keys()
            .next()
            .unwrap();
        database.posit_time_lookup.lock().unwrap().remove(&posit);
        assert!(matches!(
            database.terrain(),
            Err(DatabaseError::Invariant(message))
                if message.contains("mismatched posit reverse indexes")
        ));
    }

    #[test]
    fn checked_counters_fail_closed_on_overflow() {
        assert!(matches!(
            checked_add(u64::MAX, 1, "test counter"),
            Err(DatabaseError::ResourceLimit {
                resource: "test counter",
                limit: u64::MAX
            })
        ));
        assert!(matches!(
            checked_mul(u64::MAX, 2, "test counter"),
            Err(DatabaseError::ResourceLimit {
                resource: "test counter",
                limit: u64::MAX
            })
        ));
    }

    #[test]
    fn each_aggregation_limit_is_fail_closed() {
        let database = Database::new(PersistenceMode::InMemory).unwrap();
        Engine::new(&database)
            .execute(
                "add role a, left, right; \
                 add posit [{(+x, a)}, 1, '2024-01-01'], \
                           [{(+l, left), (+r, right)}, 1, '2024-01-01'];",
            )
            .unwrap();
        for (resource, limits) in [
            (
                "active appearance sets",
                TerrainLimits {
                    active_appearance_sets: 1,
                    ..TerrainLimits::default()
                },
            ),
            (
                "endpoint Things",
                TerrainLimits {
                    endpoint_things: 1,
                    ..TerrainLimits::default()
                },
            ),
            (
                "temporal comparisons",
                TerrainLimits {
                    temporal_comparisons: 0,
                    ..TerrainLimits::default()
                },
            ),
            (
                "relationship arity",
                TerrainLimits {
                    relationship_arity: 1,
                    ..TerrainLimits::default()
                },
            ),
            (
                "allocations",
                TerrainLimits {
                    allocations: 0,
                    ..TerrainLimits::default()
                },
            ),
        ] {
            assert!(matches!(
                database.terrain_with_options(TerrainOptions {
                    as_of: Some(fixed_time("2025-01-01")),
                    limits,
                    ..TerrainOptions::default()
                }),
                Err(DatabaseError::ResourceLimit { resource: actual, .. }) if actual == resource
            ));
        }
    }
}
