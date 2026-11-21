use super::budget::{BudgetVec, Reservation, Workspace, sort_work};
use super::*;
use crate::construct::{Appearance, AppearanceSet, Posit, Role};
use crate::effect::{EffectCut, EffectiveSlice};
use pest::iterators::Pair;
use roaring::RoaringTreemap;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use unicode_normalization::UnicodeNormalization;

mod calendar;
mod json;
mod pipeline;
pub(super) mod plan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Domain {
    Thing,
    Role,
    Posit,
    AppearanceSet,
    Literal,
    Time,
}

impl Domain {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Thing => "Thing",
            Self::Role => "Role",
            Self::Posit => "Posit",
            Self::AppearanceSet => "AppearanceSet",
            Self::Literal => "LiteralValue",
            Self::Time => "Time",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum BoundValue {
    Thing(Thing),
    Role(Arc<Role>),
    Posit(Thing),
    AppearanceSet(Arc<AppearanceSet>),
    Literal(LiteralValue),
    Time(Time),
}

impl BoundValue {
    fn domain(&self) -> Domain {
        match self {
            Self::Thing(_) => Domain::Thing,
            Self::Role(_) => Domain::Role,
            Self::Posit(_) => Domain::Posit,
            Self::AppearanceSet(_) => Domain::AppearanceSet,
            Self::Literal(_) => Domain::Literal,
            Self::Time(_) => Domain::Time,
        }
    }

    fn result_cell(&self) -> ResultCell {
        match self {
            Self::Thing(identity) => ResultCell::new(ResultCellKind::Thing, identity.to_string()),
            Self::Role(role) => ResultCell::new(ResultCellKind::Role, role.name()),
            Self::Posit(identity) => ResultCell::new(ResultCellKind::Posit, identity.to_string()),
            Self::AppearanceSet(set) => {
                ResultCell::new(ResultCellKind::AppearanceSet, set.to_string())
            }
            Self::Literal(literal) => ResultCell::new(ResultCellKind::Literal, literal.token()),
            Self::Time(time) => ResultCell::new(ResultCellKind::Time, time.to_string()),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
struct Binding(BTreeMap<String, BoundValue>);

impl Workspace for BoundValue {
    fn heap_bytes(&self) -> u64 {
        match self {
            Self::Literal(value) => value.token().len() as u64,
            _ => 0,
        }
    }
}

impl Workspace for Binding {
    fn heap_bytes(&self) -> u64 {
        self.0
            .iter()
            .map(|(name, value)| {
                (size_of::<(String, BoundValue)>() as u64 + 48)
                    .saturating_add(name.len() as u64)
                    .saturating_add(value.heap_bytes())
            })
            .fold(0, u64::saturating_add)
    }
}

impl Workspace for ResultCell {
    fn heap_bytes(&self) -> u64 {
        self.text.capacity() as u64
    }
}

fn projection_bytes(binding: &Binding, columns: &[String]) -> u64 {
    columns
        .iter()
        .map(|name| {
            let text = match binding.get(name) {
                Some(BoundValue::Thing(_) | BoundValue::Posit(_)) => 20,
                Some(BoundValue::Role(role)) => role.name().len() as u64,
                Some(BoundValue::AppearanceSet(set)) => set
                    .appearances()
                    .iter()
                    .map(|a| a.role().name().len() as u64 + 32)
                    .fold(2, u64::saturating_add),
                Some(BoundValue::Literal(value)) => value.token().len() as u64,
                Some(BoundValue::Time(_)) => 64,
                None => 0,
            };
            (size_of::<ResultCell>() as u64).saturating_add(text)
        })
        .fold(0, u64::saturating_add)
}

impl Binding {
    fn get(&self, name: &str) -> Option<&BoundValue> {
        self.0.get(name)
    }

    fn bind(&mut self, name: &str, value: BoundValue) -> Result<bool, DatabaseError> {
        let Some(existing) = self.0.get(name) else {
            self.0.insert(name.to_string(), value);
            return Ok(true);
        };
        match (existing, &value) {
            (BoundValue::Thing(left), BoundValue::Posit(right)) => {
                if left == right {
                    self.0.insert(name.to_string(), value);
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            (BoundValue::Posit(left), BoundValue::Thing(right)) => Ok(left == right),
            _ if existing.domain() != value.domain() => Err(DatabaseError::VariableDomain {
                name: name.to_string(),
                first: existing.domain().label(),
                second: value.domain().label(),
            }),
            _ => Ok(existing == &value),
        }
    }
}

#[derive(Debug, Clone)]
enum ThingSlot {
    Wildcard,
    Identity(Thing),
    Variable(String),
    LegacyVariable(String),
    OneOf(Vec<String>),
}

#[derive(Debug, Clone)]
enum RoleSlot {
    Wildcard,
    Named(String),
    Variable(String),
}

#[derive(Debug, Clone)]
struct AppearancePattern {
    thing: ThingSlot,
    role: RoleSlot,
}

#[derive(Debug, Clone)]
struct AppearanceSetPattern {
    binding: Option<String>,
    members: Vec<AppearancePattern>,
    open: bool,
    any: bool,
}

impl AppearanceSetPattern {
    // Every set in an index group has the same roles. Variables and identities
    // are still matched against the individual sets selected from the group.
    fn accepts_roles(&self, set: &AppearanceSet) -> bool {
        if self.any {
            return true;
        }
        let roles = set.appearances();
        let size_matches = if self.open {
            roles.len() >= self.members.len()
        } else {
            roles.len() == self.members.len()
        };
        size_matches
            && self.members.iter().all(|member| match &member.role {
                RoleSlot::Named(name) => roles.iter().any(|a| a.role_name() == name),
                _ => true,
            })
    }
}

#[derive(Debug, Clone)]
enum ValueSlot {
    Wildcard,
    Variable(String),
    LegacyVariable(String),
    Literal(LiteralValue),
}

#[derive(Debug, Clone)]
enum TimeSlot {
    Wildcard,
    Variable(String),
    LegacyVariable(String),
    Literal(Time),
}

#[derive(Debug, Clone)]
enum Cutoff {
    Literal(Time),
    Variable(String),
}

#[derive(Debug, Clone)]
struct PositPattern {
    posit: Option<String>,
    appearances: AppearanceSetPattern,
    value: ValueSlot,
    time: TimeSlot,
    cutoff: Option<Cutoff>,
    latest_matching: bool,
    effect: Option<EffectPattern>,
}

#[derive(Debug, Clone)]
struct EffectPattern {
    assertion_cutoff: Cutoff,
    appearance_cutoff: Cutoff,
    via: Option<Box<PositPattern>>,
}

#[derive(Debug, Clone)]
enum Operand {
    Variable(String),
    Literal(LiteralValue),
    Time(Time),
    Parameter(String),
    Identity(BoundValue),
}

#[derive(Debug, Clone)]
enum Predicate {
    Comparison {
        left: String,
        operator: String,
        right: Operand,
    },
    Calendar(calendar::Test),
    Json(json::Test),
}

impl Predicate {
    fn variables(&self) -> Vec<&str> {
        match self {
            Self::Comparison { left, right, .. } => {
                let mut vars = vec![left.as_str()];
                if let Operand::Variable(name) = right {
                    vars.push(name);
                }
                vars
            }
            Self::Calendar(test) => test.variables.iter().map(String::as_str).collect(),
            Self::Json(test) => test.variables(),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct OrderItem {
    pub variable: String,
    pub descending: bool,
}

#[derive(Debug)]
struct Absence {
    patterns: Vec<PositPattern>,
    predicates: Vec<Predicate>,
}

#[derive(Debug)]
struct Query {
    branches: Vec<Vec<PositPattern>>,
    not_exists: Vec<Absence>,
    predicates: Vec<Predicate>,
    returns: Vec<String>,
    json_projections: Vec<(String, json::Call)>,
    distinct: bool,
    order: Vec<OrderItem>,
    limit: Option<usize>,
    has_fallback: bool,
}

#[derive(Debug)]
pub(super) struct PreparedQuery {
    query: Query,
    domains: HashMap<String, Domain>,
}

impl PreparedQuery {
    /// Include local variables while validating a correlated absence filter.
    pub(super) fn absence_domains(
        &self,
        index: usize,
    ) -> Result<HashMap<String, Domain>, DatabaseError> {
        let mut domains = self.domains.clone();
        for pattern in &self.query.not_exists[index].patterns {
            register_pattern(pattern, &mut domains)?;
        }
        Ok(domains)
    }

    pub(super) fn domains(&self) -> &HashMap<String, Domain> {
        &self.domains
    }

    pub(super) fn returns(&self) -> &[String] {
        &self.query.returns
    }

    pub(super) fn return_domain(&self, name: &str) -> Domain {
        // Computed JSON columns are the only projections without a pattern binding.
        self.domains.get(name).copied().unwrap_or(Domain::Literal)
    }

    pub(super) fn distinct(&self) -> bool {
        self.query.distinct
    }

    pub(super) fn order(&self) -> &[OrderItem] {
        &self.query.order
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IdentityDomain {
    Thing,
    Posit,
}

#[derive(Debug, Clone)]
pub(super) struct IdentityBinding {
    pub domain: IdentityDomain,
    pub values: ResultSet,
}

#[derive(Debug)]
pub(super) struct SearchOutcome {
    pub matched_rows: usize,
    pub identity_bindings: HashMap<String, IdentityBinding, OtherHasher>,
    pub has_return: bool,
    pub sink_stopped: bool,
    pub _identity_memory: Reservation,
}

#[derive(Clone)]
struct StructuralMatch {
    posit: Arc<Posit<LiteralValue>>,
    binding: Binding,
}

impl Workspace for StructuralMatch {
    fn heap_bytes(&self) -> u64 {
        self.binding.heap_bytes()
    }
}

struct CachedSlice {
    slice: Arc<EffectiveSlice>,
    _memory: Reservation,
}

type EffectCache = HashMap<EffectCut, CachedSlice>;

pub(super) fn execute(
    view: &ReadView,
    command: Pair<'_, Rule>,
    sink: &mut dyn RowSink,
    return_columns: &mut Option<Vec<String>>,
    execution: &ExecutionContext,
) -> Result<SearchOutcome, DatabaseError> {
    let prepared = prepare(command, execution)?;
    execute_prepared(view, prepared, sink, return_columns, execution)
}

pub(super) fn prepare(
    command: Pair<'_, Rule>,
    execution: &ExecutionContext,
) -> Result<PreparedQuery, DatabaseError> {
    execution.check()?;
    let query = parse_query(command, execution)?;
    let domains = validate_domains(&query)?;
    Ok(PreparedQuery { query, domains })
}

pub(super) fn execute_prepared(
    view: &ReadView,
    prepared: PreparedQuery,
    sink: &mut dyn RowSink,
    return_columns: &mut Option<Vec<String>>,
    execution: &ExecutionContext,
) -> Result<SearchOutcome, DatabaseError> {
    execution.check()?;
    let planned = plan::compile(view, prepared, execution)?;
    execute_plan(view, planned, sink, return_columns, execution)
}

pub(super) fn explain_prepared(
    view: &ReadView,
    prepared: PreparedQuery,
    execution: &ExecutionContext,
) -> Result<plan::QueryPlan, DatabaseError> {
    let planned = plan::compile(view, prepared, execution)?;
    execution.check()?;
    Ok(planned.describe(execution))
}

fn execute_plan(
    view: &ReadView,
    query: plan::PhysicalQuery,
    sink: &mut dyn RowSink,
    return_columns: &mut Option<Vec<String>>,
    execution: &ExecutionContext,
) -> Result<SearchOutcome, DatabaseError> {
    execution.work(1)?;
    if query.materialization_reason.is_none() {
        pipeline::execute(view, &query, sink, return_columns, execution)
    } else {
        execute_materialized(view, query, sink, return_columns, execution)
    }
}

// Keep the evaluation barriers for data-dependent failures and mutation scope.
// Moving a filter or stopping a join in these plans could hide a comparison
// error, invalid evidence, or identities needed by `search ... or add`.
fn execute_materialized(
    view: &ReadView,
    query: plan::PhysicalQuery,
    sink: &mut dyn RowSink,
    return_columns: &mut Option<Vec<String>>,
    execution: &ExecutionContext,
) -> Result<SearchOutcome, DatabaseError> {
    let domains = &query.domains;
    let mut effect_cache = EffectCache::new();

    let mut bindings = BudgetVec::new(&execution.budget);
    for branch in &query.branches {
        let mut branch_bindings = BudgetVec::new(&execution.budget);
        branch_bindings.push(branch.anchors.clone())?;
        for pattern in &branch.patterns {
            let mut joined = BudgetVec::new(&execution.budget);
            for binding in &branch_bindings {
                execution.work(1)?;
                joined.extend(evaluate_pattern(
                    view,
                    pattern,
                    binding,
                    domains,
                    execution,
                    &mut effect_cache,
                )?)?;
            }
            branch_bindings = joined;
            if branch_bindings.is_empty() {
                break;
            }
        }
        for mut binding in branch_bindings {
            // Union branches export only their common schema. Private bindings
            // must not enter correlations or later mutation-variable promotion.
            binding.0.retain(|name, _| domains.contains_key(name));
            bindings.push(binding)?;
        }
    }

    for absence in &query.not_exists {
        let mut retained = BudgetVec::new(&execution.budget);
        for binding in bindings {
            execution.work(1)?;
            let _working = execution.workspace(|| binding.heap_bytes().saturating_add(128))?;
            if !block_has_match(
                absence,
                &binding,
                view,
                domains,
                execution,
                &mut effect_cache,
            )? {
                retained.push(binding)?;
            }
        }
        bindings = retained;
    }

    for predicate in &query.predicates {
        let mut selected = BudgetVec::new(&execution.budget);
        for binding in bindings {
            execution.work(1)?;
            let _working = execution.workspace(|| binding.heap_bytes().saturating_add(128))?;
            if evaluate_predicate(predicate, &binding, execution)? {
                selected.push(binding)?;
            }
        }
        bindings = selected;
    }

    let matched_rows = bindings.len();
    let mut identity_bindings: HashMap<String, IdentityBinding, OtherHasher> = HashMap::default();
    let mut identity_memory = execution.workspace(|| 0)?;
    for binding in &bindings {
        for (name, value) in &binding.0 {
            execution.work(1)?;
            let (domain, identity) = match value {
                BoundValue::Thing(identity) => (IdentityDomain::Thing, *identity),
                BoundValue::Posit(identity) => (IdentityDomain::Posit, *identity),
                BoundValue::Role(_)
                | BoundValue::AppearanceSet(_)
                | BoundValue::Literal(_)
                | BoundValue::Time(_) => continue,
            };
            match identity_bindings.entry(name.clone()) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    identity_memory.grow(512 + name.len() as u64)?;
                    let mut values = ResultSet::new();
                    values.insert(identity);
                    entry.insert(IdentityBinding { domain, values });
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    let values = &entry.get().values;
                    if values.thing != Some(identity)
                        && !values
                            .multi
                            .as_ref()
                            .is_some_and(|values| values.contains(identity))
                    {
                        identity_memory.grow(256)?;
                    }
                    if entry.get().domain != domain {
                        return Err(DatabaseError::VariableDomain {
                            name: name.clone(),
                            first: match entry.get().domain {
                                IdentityDomain::Thing => "Thing",
                                IdentityDomain::Posit => "Posit",
                            },
                            second: match domain {
                                IdentityDomain::Thing => "Thing",
                                IdentityDomain::Posit => "Posit",
                            },
                        });
                    }
                    entry.get_mut().values.insert(identity);
                }
            }
        }
    }

    let has_return = !query.returns.is_empty();
    // A miss can be retried on a newer view. Its caller presents metadata
    // exactly once, after the final decision and before executing the fallback.
    if !has_return || (query.has_fallback && matched_rows == 0) {
        if has_return {
            *return_columns = Some(query.returns.clone());
        }
        return Ok(SearchOutcome {
            matched_rows,
            identity_bindings,
            has_return,
            sink_stopped: false,
            _identity_memory: identity_memory,
        });
    }

    let mut projected = BudgetVec::new(&execution.budget);
    for mut binding in bindings {
        execution.work(1)?;
        let _working = execution.workspace(|| binding.heap_bytes().saturating_add(128))?;
        let _computed = json::bind_projections(&mut binding, &query.json_projections, execution)?;
        let _row = execution.workspace(|| projection_bytes(&binding, &query.returns))?;
        let row = query
            .returns
            .iter()
            .map(|name| {
                binding
                    .get(name)
                    .map(BoundValue::result_cell)
                    .ok_or_else(|| DatabaseError::UnknownVariable(name.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        projected.push((binding, row))?;
    }
    if query.distinct {
        let mut seen = std::collections::BTreeSet::new();
        let mut seen_memory = execution.workspace(|| 0)?;
        let mut unique = BudgetVec::new(&execution.budget);
        for (binding, row) in projected {
            execution.work(1)?;
            if !seen.contains(&row) {
                seen_memory.grow(row.heap_bytes().saturating_add(96))?;
                seen.insert(row.clone());
                unique.push((binding, row))?;
            }
        }
        projected = unique;
    }
    if !query.order.is_empty() {
        execution.work((projected.len() as u64).saturating_mul(query.order.len() as u64))?;
        validate_order_values(&projected, &query.order)?;
        execution.work(sort_work(projected.len(), query.order.len()))?;
        let _scratch = execution.workspace(|| {
            (projected.len() as u64).saturating_mul(size_of::<(Binding, Vec<ResultCell>)>() as u64)
        })?;
        projected.sort_by(|(left, _), (right, _)| compare_order(left, right, &query.order));
    }

    let columns = query.returns.clone();
    *return_columns = Some(columns.clone());
    if let SinkFlow::Stop = sink.on_meta(&columns) {
        return Ok(SearchOutcome {
            matched_rows,
            identity_bindings,
            has_return,
            sink_stopped: true,
            _identity_memory: identity_memory,
        });
    }
    let mut sink_stopped = false;
    for (_, row) in projected {
        execution.work(1)?;
        if let SinkFlow::Stop = sink.push(row) {
            sink_stopped = true;
            break;
        }
    }
    Ok(SearchOutcome {
        matched_rows,
        identity_bindings,
        has_return,
        sink_stopped,
        _identity_memory: identity_memory,
    })
}

fn parse_query(
    command: Pair<'_, Rule>,
    execution: &ExecutionContext,
) -> Result<Query, DatabaseError> {
    let limit = search_limit(&command)?;
    let mut branches = Vec::new();
    let mut not_exists = Vec::new();
    let mut predicates = Vec::new();
    let mut returns = Vec::new();
    let mut json_projections = Vec::new();
    let mut distinct = false;
    let mut order = Vec::new();
    let mut has_fallback = false;
    for clause in command.into_inner() {
        match clause.as_rule() {
            Rule::search_clause | Rule::union_clause => {
                let mut branch = Vec::new();
                for pattern in clause.into_inner() {
                    if pattern.as_rule() == Rule::posit_search {
                        branch.push(parse_pattern(pattern, execution)?);
                    }
                }
                branches.push(branch);
            }
            Rule::not_exists_clause => {
                let mut block = Absence {
                    patterns: Vec::new(),
                    predicates: Vec::new(),
                };
                for part in clause.into_inner() {
                    if part.as_rule() == Rule::where_clause {
                        for condition in part.into_inner() {
                            block
                                .predicates
                                .push(parse_predicate(condition, execution)?);
                        }
                    } else {
                        block.patterns.push(parse_pattern(part, execution)?);
                    }
                }
                not_exists.push(block);
            }
            Rule::where_clause => {
                for condition in clause.into_inner() {
                    if condition.as_rule() == Rule::condition {
                        predicates.push(parse_predicate(condition, execution)?);
                    }
                }
            }
            Rule::return_clause => {
                for variable in clause.into_inner() {
                    match variable.as_rule() {
                        Rule::distinct => distinct = true,
                        Rule::json_projection => {
                            let mut parts = variable.into_inner();
                            let call = json::Call::parse(parts.next().unwrap(), execution)?;
                            let name = parts
                                .next()
                                .map(|p| variable_name(&p))
                                .unwrap_or_else(|| call.source.clone());
                            returns.push(name.clone());
                            json_projections.push((name, call));
                        }
                        Rule::variable_reference
                        | Rule::query_variable
                        | Rule::recall
                        | Rule::insert => returns.push(variable_name(&variable)),
                        rule => {
                            return Err(DatabaseError::Invariant(format!(
                                "unexpected return component {rule:?}"
                            )));
                        }
                    }
                }
            }
            Rule::order_by_clause => {
                for item in clause.into_inner() {
                    if item.as_rule() != Rule::order_item {
                        continue;
                    }
                    let mut variable = None;
                    let mut descending = false;
                    for part in item.into_inner() {
                        match part.as_rule() {
                            Rule::variable_reference
                            | Rule::query_variable
                            | Rule::recall
                            | Rule::insert => variable = Some(variable_name(&part)),
                            Rule::direction => {
                                descending = part.as_str().eq_ignore_ascii_case("desc")
                            }
                            rule => {
                                return Err(DatabaseError::Invariant(format!(
                                    "unexpected order component {rule:?}"
                                )));
                            }
                        }
                    }
                    order.push(OrderItem {
                        variable: variable.ok_or_else(|| {
                            DatabaseError::Invariant("order item has no variable".into())
                        })?,
                        descending,
                    });
                }
            }
            Rule::limit_clause => {}
            Rule::or_add_clause => has_fallback = true,
            rule => {
                return Err(DatabaseError::Invariant(format!(
                    "unexpected query clause {rule:?}"
                )));
            }
        }
    }
    if branches.first().is_none_or(Vec::is_empty) {
        return Err(DatabaseError::Execution(
            "a search requires at least one posit pattern".into(),
        ));
    }
    Ok(Query {
        branches,
        not_exists,
        predicates,
        returns,
        json_projections,
        distinct,
        order,
        limit,
        has_fallback,
    })
}

fn parse_pattern(
    pattern: Pair<'_, Rule>,
    execution: &ExecutionContext,
) -> Result<PositPattern, DatabaseError> {
    let mut posit = None;
    let mut appearances = None;
    let mut value = None;
    let mut time = None;
    let mut cutoff = None;
    let mut effect = None;
    let mut via = None;
    let mut latest_matching = false;
    let mut legacy_syntax = false;
    for component in pattern.into_inner() {
        match component.as_rule() {
            Rule::insert | Rule::recall if appearances.is_none() => {
                posit = Some(variable_name(&component));
                legacy_syntax = true;
            }
            Rule::posit_binding => {
                posit = Some(variable_name(&component));
            }
            Rule::latest_marker => {
                latest_matching = true;
            }
            Rule::appearance_set_search => {
                appearances = Some(parse_appearance_set(component, execution)?);
            }
            Rule::appearing_value_search => {
                value = Some(parse_value_slot(component, execution)?);
            }
            Rule::appearance_time_search => {
                time = Some(parse_time_slot(component, execution)?);
            }
            Rule::as_of_clause => {
                let component = component
                    .into_inner()
                    .next()
                    .ok_or_else(|| DatabaseError::Invariant("as-of clause has no cutoff".into()))?;
                cutoff = Some(parse_cutoff(component, execution, "as-of")?);
            }
            Rule::in_effect_clause => {
                let mut cuts = component.into_inner();
                let assertion_cutoff = cuts.next().ok_or_else(|| {
                    DatabaseError::Invariant("in-effect clause has no assertion cutoff".into())
                })?;
                let appearance_cutoff = cuts.next().ok_or_else(|| {
                    DatabaseError::Invariant("in-effect clause has no appearance cutoff".into())
                })?;
                effect = Some(EffectPattern {
                    assertion_cutoff: parse_cutoff(
                        assertion_cutoff,
                        execution,
                        "in-effect assertion",
                    )?,
                    appearance_cutoff: parse_cutoff(
                        appearance_cutoff,
                        execution,
                        "in-effect appearance",
                    )?,
                    via: None,
                });
            }
            Rule::via_clause => {
                let via_pattern = component.into_inner().next().ok_or_else(|| {
                    DatabaseError::Invariant("via clause has no assertion pattern".into())
                })?;
                via = Some(parse_pattern(via_pattern, execution)?);
            }
            rule => {
                return Err(DatabaseError::Invariant(format!(
                    "unexpected posit-pattern component {rule:?}"
                )));
            }
        }
    }
    let mut appearances = appearances.ok_or_else(|| {
        DatabaseError::Invariant("posit pattern has no appearance-set slot".into())
    })?;
    legacy_syntax |= appearances
        .members
        .iter()
        .any(AppearancePattern::uses_legacy_syntax);
    legacy_syntax |= matches!(&value, Some(ValueSlot::LegacyVariable(_)));
    legacy_syntax |= matches!(&time, Some(TimeSlot::LegacyVariable(_)));
    if legacy_syntax && !appearances.open {
        appearances.open = true;
    }
    if latest_matching && effect.is_some() {
        return Err(DatabaseError::Execution(
            "latest cannot be combined with in effect".into(),
        ));
    }
    if let Some(via) = via {
        let effect = effect.as_mut().ok_or_else(|| {
            DatabaseError::Execution("via requires an in effect target pattern".into())
        })?;
        validate_via_pattern(&via)?;
        effect.via = Some(Box::new(via));
    }
    let parsed = PositPattern {
        posit,
        appearances,
        value: value
            .ok_or_else(|| DatabaseError::Invariant("posit pattern has no value slot".into()))?,
        time: time
            .ok_or_else(|| DatabaseError::Invariant("posit pattern has no time slot".into()))?,
        cutoff,
        latest_matching,
        effect,
    };
    Ok(parsed)
}

fn parse_cutoff(
    component: Pair<'_, Rule>,
    execution: &ExecutionContext,
    label: &str,
) -> Result<Cutoff, DatabaseError> {
    if component.as_rule() == Rule::effect_cut_operand {
        return parse_cutoff(
            component
                .into_inner()
                .next()
                .ok_or_else(|| DatabaseError::Invariant(format!("{label} cutoff is empty")))?,
            execution,
            label,
        );
    }
    match component.as_rule() {
        Rule::query_variable => Ok(Cutoff::Variable(variable_name(&component))),
        Rule::constant | Rule::time => Ok(Cutoff::Literal(
            parse_time_with_now(component.as_str(), &execution.metadata.resolved_now).ok_or_else(
                || {
                    DatabaseError::Execution(format!(
                        "invalid {label} cutoff '{}'",
                        component.as_str()
                    ))
                },
            )?,
        )),
        Rule::parameter => Ok(Cutoff::Literal(
            execution
                .parameter(component.as_str())?
                .time()
                .ok_or_else(|| {
                    DatabaseError::Parameter(format!(
                        "{} is a literal parameter, not a time parameter",
                        component.as_str()
                    ))
                })?,
        )),
        rule => Err(DatabaseError::Invariant(format!(
            "unexpected {label} cutoff {rule:?}"
        ))),
    }
}

fn validate_via_pattern(pattern: &PositPattern) -> Result<(), DatabaseError> {
    if pattern.cutoff.is_some() || pattern.effect.is_some() || pattern.latest_matching {
        return Err(DatabaseError::Execution(
            "via must be a plain assertion-envelope pattern".into(),
        ));
    }
    if pattern.appearances.any || pattern.appearances.open || pattern.appearances.members.len() != 2
    {
        return Err(DatabaseError::Execution(
            "via requires the exact {posit, ascertains} assertion shape".into(),
        ));
    }
    let mut roles = pattern
        .appearances
        .members
        .iter()
        .map(|member| match &member.role {
            RoleSlot::Named(name) => Ok(name.as_str()),
            _ => Err(DatabaseError::Execution(
                "via assertion roles must be the literal roles posit and ascertains".into(),
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    roles.sort_unstable();
    if roles != ["ascertains", "posit"] {
        return Err(DatabaseError::Execution(
            "via requires the exact {posit, ascertains} assertion shape".into(),
        ));
    }
    Ok(())
}

fn parse_appearance_set(
    pattern: Pair<'_, Rule>,
    execution: &ExecutionContext,
) -> Result<AppearanceSetPattern, DatabaseError> {
    let mut members = Vec::new();
    let mut any = false;
    let mut binding = None;
    let mut open = false;
    for member in pattern.into_inner() {
        match member.as_rule() {
            Rule::wildcard => any = true,
            Rule::appearance_search => members.push(parse_appearance(member, execution)?),
            Rule::appearance_set_binding => binding = Some(variable_name(&member)),
            Rule::appearance_list => {
                for item in member.into_inner() {
                    match item.as_rule() {
                        Rule::appearance_search => members.push(parse_appearance(item, execution)?),
                        Rule::ellipsis => open = true,
                        rule => {
                            return Err(DatabaseError::Invariant(format!(
                                "unexpected appearance-list member {rule:?}"
                            )));
                        }
                    }
                }
            }
            Rule::ellipsis => open = true,
            rule => {
                return Err(DatabaseError::Invariant(format!(
                    "unexpected appearance-set member {rule:?}"
                )));
            }
        }
    }
    Ok(AppearanceSetPattern {
        binding,
        members,
        open,
        any,
    })
}

fn parse_appearance(
    appearance: Pair<'_, Rule>,
    execution: &ExecutionContext,
) -> Result<AppearancePattern, DatabaseError> {
    let mut inner = appearance.into_inner();
    let thing = inner
        .next()
        .ok_or_else(|| DatabaseError::Invariant("appearance lacks a Thing slot".into()))?;
    let role = inner
        .next()
        .ok_or_else(|| DatabaseError::Invariant("appearance lacks a Role slot".into()))?;
    let thing = match thing.as_rule() {
        Rule::wildcard => ThingSlot::Wildcard,
        // Definition validation infers a signature before there are caller values.
        Rule::parameter if execution.validating_definition => ThingSlot::Identity(0),
        Rule::parameter => ThingSlot::Identity(
            execution
                .parameter(thing.as_str())?
                .identity(thing.as_str())?
                .1,
        ),
        Rule::query_variable => ThingSlot::Variable(variable_name(&thing)),
        Rule::insert | Rule::recall => ThingSlot::LegacyVariable(variable_name(&thing)),
        Rule::recall_union | Rule::query_union => ThingSlot::OneOf(
            thing
                .into_inner()
                .filter(|part| matches!(part.as_rule(), Rule::recall | Rule::query_variable))
                .map(|part| variable_name(&part))
                .collect(),
        ),
        rule => {
            return Err(DatabaseError::Invariant(format!(
                "unexpected Thing slot {rule:?}"
            )));
        }
    };
    let role = match role.as_rule() {
        Rule::wildcard => RoleSlot::Wildcard,
        Rule::role => RoleSlot::Named(canonical_role_name(role.as_str())),
        Rule::query_variable | Rule::insert | Rule::recall => {
            RoleSlot::Variable(variable_name(&role))
        }
        rule => {
            return Err(DatabaseError::Invariant(format!(
                "unexpected Role slot {rule:?}"
            )));
        }
    };
    Ok(AppearancePattern { thing, role })
}

fn parse_value_slot(
    slot: Pair<'_, Rule>,
    execution: &ExecutionContext,
) -> Result<ValueSlot, DatabaseError> {
    let value = slot
        .into_inner()
        .next()
        .ok_or_else(|| DatabaseError::Invariant("value pattern is empty".into()))?;
    match value.as_rule() {
        Rule::wildcard => Ok(ValueSlot::Wildcard),
        Rule::query_variable => Ok(ValueSlot::Variable(variable_name(&value))),
        Rule::insert | Rule::recall => Ok(ValueSlot::LegacyVariable(variable_name(&value))),
        Rule::parameter => Ok(ValueSlot::Literal(
            execution.parameter(value.as_str())?.literal()?,
        )),
        rule => parse_lossless_literal(rule, value.as_str(), &execution.metadata.resolved_now)
            .map(ValueSlot::Literal),
    }
}

fn parse_time_slot(
    slot: Pair<'_, Rule>,
    execution: &ExecutionContext,
) -> Result<TimeSlot, DatabaseError> {
    let time = slot
        .into_inner()
        .next()
        .ok_or_else(|| DatabaseError::Invariant("time pattern is empty".into()))?;
    match time.as_rule() {
        Rule::wildcard => Ok(TimeSlot::Wildcard),
        Rule::query_variable => Ok(TimeSlot::Variable(variable_name(&time))),
        Rule::insert | Rule::recall => Ok(TimeSlot::LegacyVariable(variable_name(&time))),
        Rule::parameter => execution
            .parameter(time.as_str())?
            .time()
            .map(TimeSlot::Literal)
            .ok_or_else(|| {
                DatabaseError::Parameter(format!(
                    "{} is a literal parameter, not a time parameter",
                    time.as_str()
                ))
            }),
        Rule::constant | Rule::time => {
            parse_time_with_now(time.as_str(), &execution.metadata.resolved_now)
                .map(TimeSlot::Literal)
                .ok_or_else(|| {
                    DatabaseError::Execution(format!("invalid time literal '{}'", time.as_str()))
                })
        }
        rule => Err(DatabaseError::Invariant(format!(
            "unexpected time slot {rule:?}"
        ))),
    }
}

fn parse_predicate(
    condition: Pair<'_, Rule>,
    execution: &ExecutionContext,
) -> Result<Predicate, DatabaseError> {
    let mut parts = condition.into_inner();
    let left = parts
        .next()
        .ok_or_else(|| DatabaseError::Invariant("predicate has no left operand".into()))?;
    if left.as_rule() == Rule::calendar_test {
        return calendar::Test::parse(left, execution).map(Predicate::Calendar);
    }
    if left.as_rule() == Rule::json_test {
        return json::Test::parse(left, execution).map(Predicate::Json);
    }
    let operator = parts
        .next()
        .ok_or_else(|| DatabaseError::Invariant("predicate has no operator".into()))?;
    let right = parts
        .next()
        .ok_or_else(|| DatabaseError::Invariant("predicate has no right operand".into()))?;
    Ok(Predicate::Comparison {
        left: variable_name(&left),
        operator: operator.as_str().to_string(),
        right: parse_operand(right, execution)?,
    })
}

fn parse_operand(
    operand: Pair<'_, Rule>,
    execution: &ExecutionContext,
) -> Result<Operand, DatabaseError> {
    if matches!(
        operand.as_rule(),
        Rule::rhs_value | Rule::variable_reference
    ) {
        return parse_operand(
            operand
                .into_inner()
                .next()
                .ok_or_else(|| DatabaseError::Invariant("empty right operand".into()))?,
            execution,
        );
    }
    match operand.as_rule() {
        Rule::query_variable | Rule::recall | Rule::insert => {
            Ok(Operand::Variable(variable_name(&operand)))
        }
        Rule::parameter if execution.validating_definition => {
            Ok(Operand::Parameter(operand.as_str().to_owned()))
        }
        Rule::parameter => match execution.parameter(operand.as_str())? {
            ResolvedParameter::Identity { kind, id, .. } => {
                Ok(Operand::Identity(if *kind == ResultCellKind::Posit {
                    BoundValue::Posit(*id)
                } else {
                    BoundValue::Thing(*id)
                }))
            }
            ResolvedParameter::Literal(literal) => Ok(Operand::Literal(literal.clone())),
            ResolvedParameter::Time { time, .. } => Ok(Operand::Time(time.clone())),
        },
        Rule::constant | Rule::time => {
            parse_time_with_now(operand.as_str(), &execution.metadata.resolved_now)
                .map(Operand::Time)
                .ok_or_else(|| {
                    DatabaseError::Execution(format!("invalid time literal '{}'", operand.as_str()))
                })
        }
        rule => parse_lossless_literal(
            rule,
            operand.as_str().trim(),
            &execution.metadata.resolved_now,
        )
        .map(Operand::Literal),
    }
}

fn variable_name(variable: &Pair<'_, Rule>) -> String {
    if matches!(
        variable.as_rule(),
        Rule::variable_reference
            | Rule::posit_binding
            | Rule::appearance_set_binding
            | Rule::order_item
    ) && let Some(inner) = variable.clone().into_inner().next()
    {
        return variable_name(&inner);
    }
    variable
        .as_str()
        .trim()
        .trim_start_matches(['+', '?'])
        .to_string()
}

fn canonical_role_name(token: &str) -> String {
    let token = token.trim();
    let token = token
        .strip_prefix('`')
        .and_then(|token| token.strip_suffix('`'))
        .unwrap_or(token);
    token.nfc().collect()
}

impl AppearancePattern {
    fn uses_legacy_syntax(&self) -> bool {
        matches!(self.thing, ThingSlot::LegacyVariable(_))
    }
}

fn validate_domains(query: &Query) -> Result<HashMap<String, Domain>, DatabaseError> {
    let mut all_variables = HashSet::new();
    let mut branch_domains = Vec::new();
    for branch in &query.branches {
        let mut domains = HashMap::new();
        for pattern in branch {
            register_pattern(pattern, &mut domains)?;
        }
        validate_pattern_dependencies(branch, &domains, &HashSet::new())?;
        all_variables.extend(domains.keys().cloned());
        branch_domains.push(domains);
    }
    // Only bindings supplied by every union branch may escape the positive
    // relation. Branch-private variables remain usable within their branch.
    let domains: HashMap<_, _> = branch_domains[0]
        .iter()
        .filter(|(name, _)| {
            branch_domains
                .iter()
                .all(|branch| branch.contains_key(*name))
        })
        .map(|(name, domain)| (name.clone(), *domain))
        .collect();
    for branch in &branch_domains {
        for (name, domain) in &domains {
            if branch[name] != *domain {
                return Err(DatabaseError::VariableDomain {
                    name: name.clone(),
                    first: domain.label(),
                    second: branch[name].label(),
                });
            }
        }
    }
    let available: HashSet<_> = domains.keys().map(String::as_str).collect();
    for block in &query.not_exists {
        let mut local_domains = domains.clone();
        for pattern in &block.patterns {
            for name in pattern.introduced().union(&pattern.dependencies()) {
                if all_variables.contains(*name) && !domains.contains_key(*name) {
                    return Err(DatabaseError::InvalidRecall(format!(
                        "correlated variable '{name}' is not bound in every union branch"
                    )));
                }
            }
            register_pattern(pattern, &mut local_domains)?;
        }
        validate_pattern_dependencies(&block.patterns, &local_domains, &available)?;
        for predicate in &block.predicates {
            validate_predicate_domains(predicate, &local_domains)?;
        }
    }
    for predicate in &query.predicates {
        validate_predicate_domains(predicate, &domains)?;
    }
    let mut computed = HashSet::new();
    for (name, call) in &query.json_projections {
        call.validate(&domains)?;
        if all_variables.contains(name) || !computed.insert(name) {
            return Err(DatabaseError::InvalidRecall(format!(
                "JSON projection name '{name}' is already in use"
            )));
        }
    }
    for returned in &query.returns {
        if !computed.contains(returned) {
            require_variable(&domains, returned)?;
        }
    }
    for item in &query.order {
        if !computed.contains(&item.variable) {
            require_variable(&domains, &item.variable)?;
        }
        if !query.returns.contains(&item.variable) {
            return Err(DatabaseError::InvalidRecall(format!(
                "ORDER BY variable '{}' must be projected",
                item.variable
            )));
        }
    }
    Ok(domains)
}

fn validate_predicate_domains(
    predicate: &Predicate,
    domains: &HashMap<String, Domain>,
) -> Result<(), DatabaseError> {
    let (left, operator, right) = match predicate {
        Predicate::Calendar(test) => return test.validate(domains),
        Predicate::Json(test) => return test.validate(domains),
        Predicate::Comparison {
            left,
            operator,
            right,
        } => (left, operator, right),
    };
    require_variable(domains, left)?;
    let left = domains[left];
    let right = match right {
        Operand::Variable(name) => {
            require_variable(domains, name)?;
            domains[name]
        }
        Operand::Parameter(_) => left,
        Operand::Identity(value) => value.domain(),
        Operand::Literal(_) => Domain::Literal,
        Operand::Time(_) => Domain::Time,
    };
    let identities = matches!(left, Domain::Thing | Domain::Posit)
        && matches!(right, Domain::Thing | Domain::Posit);
    let equality = matches!(operator.as_str(), "=" | "===" | "?=");
    if (left == right || identities) && (equality || matches!(left, Domain::Literal | Domain::Time))
    {
        Ok(())
    } else {
        Err(DatabaseError::Comparison(format!(
            "{} {} {} is unsupported",
            left.label(),
            operator,
            right.label()
        )))
    }
}

fn validate_pattern_dependencies(
    patterns: &[PositPattern],
    domains: &HashMap<String, Domain>,
    initially_available: &HashSet<&str>,
) -> Result<(), DatabaseError> {
    for pattern in patterns {
        validate_dependency_domains(pattern, domains)?;
    }
    let mut available = initially_available.clone();
    let mut pending: Vec<_> = patterns.iter().collect();
    while !pending.is_empty() {
        let Some(index) = pending
            .iter()
            .position(|p| p.dependencies().is_subset(&available))
        else {
            let mut missing: Vec<_> = pending
                .iter()
                .flat_map(|p| p.dependencies())
                .filter(|name| !available.contains(name))
                .collect();
            missing.sort_unstable();
            missing.dedup();
            return Err(DatabaseError::InvalidRecall(format!(
                "query has unresolvable variable dependencies: {}",
                missing.join(", ")
            )));
        };
        available.extend(pending.remove(index).introduced());
    }
    Ok(())
}

fn validate_dependency_domains(
    pattern: &PositPattern,
    domains: &HashMap<String, Domain>,
) -> Result<(), DatabaseError> {
    let require_domain = |name: &str, expected: Domain| {
        require_variable(domains, name)?;
        let actual = domains[name];
        if actual == expected || (expected == Domain::Thing && actual == Domain::Posit) {
            Ok(())
        } else {
            Err(DatabaseError::VariableDomain {
                name: name.to_string(),
                first: actual.label(),
                second: expected.label(),
            })
        }
    };
    if let Some(Cutoff::Variable(name)) = &pattern.cutoff {
        require_domain(name, Domain::Time)?;
    }
    if let Some(effect) = &pattern.effect {
        for cutoff in [&effect.assertion_cutoff, &effect.appearance_cutoff] {
            if let Cutoff::Variable(name) = cutoff {
                require_domain(name, Domain::Time)?;
            }
        }
        if let Some(via) = effect.via.as_deref() {
            validate_dependency_domains(via, domains)?;
        }
    }
    for member in &pattern.appearances.members {
        if let ThingSlot::OneOf(names) = &member.thing {
            for name in names {
                require_domain(name, Domain::Thing)?;
            }
        }
    }
    Ok(())
}

fn register_pattern(
    pattern: &PositPattern,
    domains: &mut HashMap<String, Domain>,
) -> Result<(), DatabaseError> {
    if let Some(name) = &pattern.posit {
        register_domain(domains, name, Domain::Posit)?;
    }
    if let Some(name) = &pattern.appearances.binding {
        register_domain(domains, name, Domain::AppearanceSet)?;
    }
    for member in &pattern.appearances.members {
        if let ThingSlot::Variable(name) | ThingSlot::LegacyVariable(name) = &member.thing {
            register_domain(domains, name, Domain::Thing)?;
        }
        if let RoleSlot::Variable(name) = &member.role {
            register_domain(domains, name, Domain::Role)?;
        }
    }
    if let ValueSlot::Variable(name) | ValueSlot::LegacyVariable(name) = &pattern.value {
        register_domain(domains, name, Domain::Literal)?;
    }
    if let TimeSlot::Variable(name) | TimeSlot::LegacyVariable(name) = &pattern.time {
        register_domain(domains, name, Domain::Time)?;
    }
    if let Some(via) = pattern
        .effect
        .as_ref()
        .and_then(|effect| effect.via.as_deref())
    {
        register_pattern(via, domains)?;
    }
    Ok(())
}

fn register_domain(
    domains: &mut HashMap<String, Domain>,
    name: &str,
    domain: Domain,
) -> Result<(), DatabaseError> {
    match domains.get(name).copied() {
        None => {
            domains.insert(name.to_string(), domain);
            Ok(())
        }
        Some(existing) if existing == domain => Ok(()),
        Some(Domain::Thing) if domain == Domain::Posit => {
            domains.insert(name.to_string(), Domain::Posit);
            Ok(())
        }
        Some(Domain::Posit) if domain == Domain::Thing => Ok(()),
        Some(existing) => Err(DatabaseError::VariableDomain {
            name: name.to_string(),
            first: existing.label(),
            second: domain.label(),
        }),
    }
}

fn require_variable(domains: &HashMap<String, Domain>, name: &str) -> Result<(), DatabaseError> {
    if domains.contains_key(name) {
        Ok(())
    } else {
        Err(DatabaseError::UnknownVariable(name.to_string()))
    }
}

impl PositPattern {
    fn introduced(&self) -> HashSet<&str> {
        let mut names = HashSet::new();
        if let Some(name) = &self.posit {
            names.insert(name.as_str());
        }
        if let Some(name) = &self.appearances.binding {
            names.insert(name.as_str());
        }
        for member in &self.appearances.members {
            if let ThingSlot::Variable(name) | ThingSlot::LegacyVariable(name) = &member.thing {
                names.insert(name.as_str());
            }
            if let RoleSlot::Variable(name) = &member.role {
                names.insert(name.as_str());
            }
        }
        if let ValueSlot::Variable(name) | ValueSlot::LegacyVariable(name) = &self.value {
            names.insert(name.as_str());
        }
        if let TimeSlot::Variable(name) | TimeSlot::LegacyVariable(name) = &self.time {
            names.insert(name.as_str());
        }
        if let Some(via) = self
            .effect
            .as_ref()
            .and_then(|effect| effect.via.as_deref())
        {
            names.extend(via.introduced());
        }
        names
    }

    fn dependencies(&self) -> HashSet<&str> {
        let mut names = HashSet::new();
        if let Some(Cutoff::Variable(name)) = &self.cutoff {
            names.insert(name.as_str());
        }
        if let Some(effect) = &self.effect {
            if let Cutoff::Variable(name) = &effect.assertion_cutoff {
                names.insert(name.as_str());
            }
            if let Cutoff::Variable(name) = &effect.appearance_cutoff {
                names.insert(name.as_str());
            }
            if let Some(via) = effect.via.as_deref() {
                names.extend(via.dependencies());
            }
        }
        for member in &self.appearances.members {
            if let ThingSlot::OneOf(union) = &member.thing {
                names.extend(union.iter().map(String::as_str));
            }
        }
        names
    }
}

fn plan_patterns<'a>(
    view: &ReadView,
    patterns: &'a [PositPattern],
    initially_available: &HashSet<&str>,
    anchors: &Binding,
) -> Result<Vec<&'a PositPattern>, DatabaseError> {
    // Keep the existing validation order for branches where reading a pattern
    // can fail. An unrelated empty literal posting must not mask that error.
    let can_reorder = patterns
        .iter()
        .all(|pattern| pattern_matches_are_total(view, pattern));
    // These bounds only use store statistics and initial parameter values.
    let mut pending: Vec<_> = patterns
        .iter()
        .map(|pattern| {
            let count = if can_reorder {
                estimated_candidate_count(view, pattern, anchors)
            } else {
                role_candidate_count(view, pattern)
            };
            (pattern, count)
        })
        .collect();
    let mut planned = Vec::with_capacity(patterns.len());
    let mut available = initially_available.clone();
    while !pending.is_empty() {
        let mut best = None;
        for (index, (pattern, count)) in pending.iter().enumerate() {
            if pattern.dependencies().is_subset(&available) {
                let connected = can_reorder
                    && (!pattern.introduced().is_disjoint(&available)
                        || !pattern.dependencies().is_empty()
                        || pattern
                            .appearances
                            .members
                            .iter()
                            .any(|m| matches!(m.thing, ThingSlot::Identity(_))));
                let estimate = (!connected, *count);
                if best.is_none_or(|(_, best_estimate)| estimate < best_estimate) {
                    best = Some((index, estimate));
                }
            }
        }
        let Some((index, _)) = best else {
            let dependencies: Vec<_> = pending
                .iter()
                .flat_map(|(pattern, _)| pattern.dependencies())
                .filter(|name| !available.contains(name))
                .collect();
            return Err(DatabaseError::InvalidRecall(format!(
                "query has unresolvable variable dependencies: {}",
                dependencies.join(", ")
            )));
        };
        let (pattern, _) = pending.remove(index);
        available.extend(pattern.introduced());
        planned.push(pattern);
    }
    Ok(planned)
}

fn pattern_value_families(view: &ReadView, pattern: &PositPattern) -> u8 {
    pattern
        .appearances
        .members
        .iter()
        .fold(view.value_families(None), |families, m| match &m.role {
            RoleSlot::Named(name) => families & view.value_families(Some(name)),
            _ => families,
        })
}

// Skipping a candidate of an incompatible family could hide a comparison error.
fn literal_comparison_is_total(
    view: &ReadView,
    pattern: &PositPattern,
    value: &LiteralValue,
) -> bool {
    pattern_value_families(view, pattern) & !value.family().nominal_mask() == 0
}

fn pattern_matches_are_total(view: &ReadView, pattern: &PositPattern) -> bool {
    pattern.effect.is_none()
        && match &pattern.value {
            ValueSlot::Literal(value) => literal_comparison_is_total(view, pattern, value),
            _ => true,
        }
}

fn indexed_literal<'a>(view: &ReadView, pattern: &'a PositPattern) -> Option<&'a LiteralValue> {
    if let ValueSlot::Literal(value) = &pattern.value
        && (pattern.cutoff.is_none() || pattern.latest_matching)
        && pattern.effect.is_none()
        && literal_comparison_is_total(view, pattern, value)
    {
        Some(value)
    } else {
        None
    }
}

fn estimated_candidate_count(view: &ReadView, pattern: &PositPattern, anchors: &Binding) -> u64 {
    let literal = indexed_literal(view, pattern).map(|v| {
        view.value_posting(v.nominal_fingerprint())
            .map_or(0, |p| p.len())
    });
    let posit = pattern
        .posit
        .as_ref()
        .filter(|_| pattern.cutoff.is_none())
        .and_then(|name| anchors.get(name))
        .map(|_| 1);
    let things = pattern.appearances.members.iter().filter_map(|m| {
        let id = match &m.thing {
            ThingSlot::Identity(id) => *id,
            ThingSlot::Variable(name) => match anchors.get(name) {
                Some(BoundValue::Thing(id) | BoundValue::Posit(id)) => *id,
                _ => return None,
            },
            _ => return None,
        };
        let role = match &m.role {
            RoleSlot::Named(name) => view.normalized_role(name),
            _ => None,
        };
        Some(view.thing_candidate_count(id, role.as_ref(), |set| {
            pattern.appearances.accepts_roles(set)
        }))
    });
    things
        .chain(literal)
        .chain(posit)
        .fold(role_candidate_count(view, pattern), u64::min)
}

fn role_candidate_count(view: &ReadView, pattern: &PositPattern) -> u64 {
    pattern
        .appearances
        .members
        .iter()
        .filter_map(|member| match &member.role {
            RoleSlot::Named(name) => Some(
                view.role(name)
                    .and_then(|role| view.role_posting(role.role()))
                    .map_or(0, RoaringTreemap::len),
            ),
            RoleSlot::Wildcard | RoleSlot::Variable(_) => None,
        })
        .min()
        .unwrap_or(u64::MAX)
}

fn evaluate_pattern(
    view: &ReadView,
    planned: &plan::PlannedPattern,
    input: &Binding,
    domains: &HashMap<String, Domain>,
    execution: &ExecutionContext,
    effect_cache: &mut EffectCache,
) -> Result<BudgetVec<Binding>, DatabaseError> {
    let pattern = &planned.pattern;
    if let Some(effect) = &pattern.effect {
        return evaluate_effect_pattern(
            view,
            pattern,
            effect,
            input,
            domains,
            execution,
            effect_cache,
        );
    }
    let cutoff = pattern
        .cutoff
        .as_ref()
        .map(|cutoff| resolve_cutoff(cutoff, input))
        .transpose()?;

    // A temporal pattern produces its own relation before joining external
    // posit/value/time bindings. Otherwise whichever pattern happens to run
    // first can hide a successor and resurrect an older matching state.
    let _reduction_memory = execution.workspace(|| {
        if pattern.cutoff.is_some() {
            input.heap_bytes().saturating_add(128)
        } else {
            0
        }
    })?;
    let reduction_input = pattern
        .cutoff
        .as_ref()
        .map(|_| temporal_input(pattern, input));
    let structural_input = reduction_input.as_ref().unwrap_or(input);
    let posits = candidate_posits(view, pattern, structural_input, &planned.access, execution)?;
    let mut structural = BudgetVec::new(&execution.budget);
    for posit in &posits {
        execution.work(1)?;
        if cutoff
            .as_ref()
            .is_some_and(|cutoff| !posit.time().definitely_at_or_before(cutoff))
        {
            continue;
        }
        for binding in match_appearance_set(
            &pattern.appearances,
            &posit.appearance_set(),
            structural_input,
            domains,
            execution,
        )? {
            structural.push(StructuralMatch {
                posit: Arc::clone(posit),
                binding,
            })?;
        }
    }

    let candidates = if pattern.cutoff.is_some() && !pattern.latest_matching {
        reduce_matches(structural, execution)?
    } else {
        structural
    };
    let mut matched = BudgetVec::new(&execution.budget);
    for candidate in candidates {
        execution.work(1)?;
        let _working = execution.workspace(|| {
            candidate
                .binding
                .heap_bytes()
                .saturating_add(candidate.posit.value().token().len() as u64)
                .saturating_add(256)
        })?;
        if let Some(binding) = match_fields(pattern, &candidate.posit, candidate.binding)? {
            matched.push(StructuralMatch {
                posit: candidate.posit,
                binding,
            })?;
        }
    }
    let matched = if pattern.cutoff.is_some() && pattern.latest_matching {
        reduce_matches(matched, execution)?
    } else {
        matched
    };
    let mut bindings = BudgetVec::new(&execution.budget);
    'row: for candidate in matched {
        execution.work(1)?;
        let _working = execution.workspace(|| {
            candidate
                .binding
                .heap_bytes()
                .saturating_add(input.heap_bytes())
                .saturating_add(256)
        })?;
        let mut binding = candidate.binding;
        for (name, value) in &input.0 {
            if !binding.bind(name, value.clone())? {
                continue 'row;
            }
        }
        bindings.push(binding)?;
    }
    Ok(bindings)
}

fn temporal_input(pattern: &PositPattern, input: &Binding) -> Binding {
    let deferred = deferred_temporal_fields(pattern);
    let mut binding = input.clone();
    binding
        .0
        .retain(|name, _| !deferred.contains(name.as_str()));
    binding
}

fn deferred_temporal_fields<'a>(pattern: &'a PositPattern) -> HashSet<&'a str> {
    if pattern.cutoff.is_none() {
        return HashSet::new();
    }
    let mut retained = pattern.dependencies();
    if let Some(name) = &pattern.appearances.binding {
        retained.insert(name);
    }
    for member in &pattern.appearances.members {
        if let ThingSlot::Variable(name) | ThingSlot::LegacyVariable(name) = &member.thing {
            retained.insert(name);
        }
        if let RoleSlot::Variable(name) = &member.role {
            retained.insert(name);
        }
    }
    let mut deferred = HashSet::new();
    let mut defer = |name: &'a str| {
        if !retained.contains(name) {
            deferred.insert(name);
        }
    };
    if let Some(name) = &pattern.posit {
        defer(name);
    }
    if let ValueSlot::Variable(name) | ValueSlot::LegacyVariable(name) = &pattern.value {
        defer(name);
    }
    if let TimeSlot::Variable(name) | TimeSlot::LegacyVariable(name) = &pattern.time {
        defer(name);
    }
    deferred
}

fn evaluate_effect_pattern(
    view: &ReadView,
    pattern: &PositPattern,
    effect: &EffectPattern,
    input: &Binding,
    domains: &HashMap<String, Domain>,
    execution: &ExecutionContext,
    effect_cache: &mut EffectCache,
) -> Result<BudgetVec<Binding>, DatabaseError> {
    let cut = EffectCut::new(
        resolve_cutoff(&effect.assertion_cutoff, input)?,
        resolve_cutoff(&effect.appearance_cutoff, input)?,
    );
    let slice = if let Some(slice) = effect_cache.get(&cut) {
        Arc::clone(&slice.slice)
    } else {
        let mut memory = execution.workspace(|| 0)?;
        let resolved = Arc::new(crate::effect::resolve_budgeted(
            view,
            cut.clone(),
            crate::effect::EffectLimits::default(),
            |units| execution.work(units),
            |bytes| memory.resize(bytes),
            |id| execution.trace_read(id),
        )?);
        memory.resize(
            (resolved.assertions().len() as u64)
                .saturating_mul(2 * size_of::<crate::effect::EffectiveAssertion>() as u64)
                .saturating_add(256),
        )?;
        effect_cache.insert(
            cut,
            CachedSlice {
                slice: Arc::clone(&resolved),
                _memory: memory,
            },
        );
        resolved
    };

    let mut matched = BudgetVec::new(&execution.budget);
    for evidence in slice.assertions() {
        execution.work(1)?;
        let target = evidence.target();
        for binding in match_appearance_set(
            &pattern.appearances,
            &target.appearance_set(),
            input,
            domains,
            execution,
        )? {
            let _working = execution.workspace(|| {
                binding
                    .heap_bytes()
                    .saturating_add(target.value().token().len() as u64)
                    .saturating_add(256)
            })?;
            let Some(binding) = match_fields(pattern, &target, binding)? else {
                continue;
            };
            let Some(via) = effect.via.as_deref() else {
                matched.push(binding)?;
                continue;
            };
            let assertion = evidence.assertion();
            for via_binding in match_appearance_set(
                &via.appearances,
                &assertion.appearance_set(),
                &binding,
                domains,
                execution,
            )? {
                let _via = execution.workspace(|| {
                    via_binding
                        .heap_bytes()
                        .saturating_add(assertion.value().token().len() as u64)
                        .saturating_add(256)
                })?;
                if let Some(via_binding) = match_fields(via, &assertion, via_binding)? {
                    matched.push(via_binding)?;
                }
            }
        }
    }
    Ok(matched)
}

fn resolve_cutoff(cutoff: &Cutoff, input: &Binding) -> Result<Time, DatabaseError> {
    match cutoff {
        Cutoff::Literal(time) => Ok(time.clone()),
        Cutoff::Variable(name) => match input.get(name) {
            Some(BoundValue::Time(time)) => Ok(time.clone()),
            Some(other) => Err(DatabaseError::VariableDomain {
                name: name.clone(),
                first: other.domain().label(),
                second: Domain::Time.label(),
            }),
            None => Err(DatabaseError::InvalidRecall(name.clone())),
        },
    }
}

fn candidate_posits(
    view: &ReadView,
    pattern: &PositPattern,
    input: &Binding,
    access: &plan::PlanAccess,
    execution: &ExecutionContext,
) -> Result<BudgetVec<Arc<Posit<LiteralValue>>>, DatabaseError> {
    let identities = candidate_identities(view, pattern, input, access, execution)?;
    let mut posits = BudgetVec::new(&execution.budget);
    for identity in identities {
        execution.work(1)?;
        execution.trace_read(identity)?;
        let posit = view.posit(identity).ok_or_else(|| {
            DatabaseError::Invariant(format!(
                "posit index contains identity {identity} without a literal posit"
            ))
        })?;
        posits.push(posit)?;
    }
    Ok(posits)
}

struct CandidateIds {
    identities: Box<dyn Iterator<Item = Thing>>,
    _memory: Reservation,
}

impl Iterator for CandidateIds {
    type Item = Thing;
    fn next(&mut self) -> Option<Thing> {
        self.identities.next()
    }
}

fn candidate_identities(
    view: &ReadView,
    pattern: &PositPattern,
    input: &Binding,
    access: &plan::PlanAccess,
    execution: &ExecutionContext,
) -> Result<CandidateIds, DatabaseError> {
    let candidates = match access {
        plan::PlanAccess::Bitmap { .. } => {
            indexed_candidate_identities(view, pattern, input, execution)?
        }
        plan::PlanAccess::FullScan => None,
        plan::PlanAccess::EffectiveSlice => {
            return Err(DatabaseError::Invariant(
                "effective slice used as a posit scan".into(),
            ));
        }
    };
    if let Some(candidates) = candidates {
        Ok(CandidateIds {
            identities: Box::new(candidates.posting.into_iter()),
            _memory: candidates.memory,
        })
    } else {
        let count = view.posit_count();
        execution.work((count as u64).saturating_add(sort_work(count, 1)))?;
        let memory =
            execution.workspace(|| (count as u64).saturating_mul(size_of::<Thing>() as u64))?;
        let mut identities = view.posit_ids().collect::<Vec<_>>();
        identities.sort_unstable();
        Ok(CandidateIds {
            identities: Box::new(identities.into_iter()),
            _memory: memory,
        })
    }
}

struct BudgetBitmap {
    posting: RoaringTreemap,
    memory: Reservation,
}

impl Workspace for BudgetBitmap {
    fn heap_bytes(&self) -> u64 {
        0
    }
}

impl std::ops::Deref for BudgetBitmap {
    type Target = RoaringTreemap;
    fn deref(&self) -> &RoaringTreemap {
        &self.posting
    }
}

fn bitmap_bytes(identities: u64) -> u64 {
    if identities == 0 {
        0
    } else {
        identities.saturating_mul(256).saturating_add(128)
    }
}

fn clone_posting(
    posting: Option<&RoaringTreemap>,
    execution: &ExecutionContext,
) -> Result<BudgetBitmap, DatabaseError> {
    let count = posting.map_or(0, RoaringTreemap::len);
    execution.work(count)?;
    let memory = execution.workspace(|| bitmap_bytes(count))?;
    Ok(BudgetBitmap {
        posting: posting.cloned().unwrap_or_default(),
        memory,
    })
}

fn indexed_candidate_identities(
    view: &ReadView,
    pattern: &PositPattern,
    input: &Binding,
    execution: &ExecutionContext,
) -> Result<Option<BudgetBitmap>, DatabaseError> {
    use crate::construct::SetPosting;
    enum Source<'a> {
        Role(Option<&'a RoaringTreemap>),
        Set(Option<&'a SetPosting>),
        Posit(Thing),
        Things(Vec<Thing>, Option<Arc<Role>>),
    }

    // Copy or expand only the smallest usable posting. The structural matcher
    // and field checks already verify the other constraints on each candidate.
    let mut best = None;
    let mut consider = |count: u64, source| {
        if best.as_ref().is_none_or(|(n, _)| count < *n) {
            best = Some((count, source));
        }
    };
    if let Some(name) = &pattern.posit
        && pattern.cutoff.is_none()
        && let Some(BoundValue::Thing(id) | BoundValue::Posit(id)) = input.get(name)
    {
        consider(u64::from(view.posit(*id).is_some()), Source::Posit(*id));
    }
    if let Some(name) = &pattern.appearances.binding
        && let Some(BoundValue::AppearanceSet(set)) = input.get(name)
    {
        let posting = view.set_posting(set);
        consider(posting.map_or(0, SetPosting::len), Source::Set(posting));
    }
    if let Some(value) = indexed_literal(view, pattern) {
        let posting = view.value_posting(value.nominal_fingerprint());
        consider(posting.map_or(0, SetPosting::len), Source::Set(posting));
    }
    for member in &pattern.appearances.members {
        execution.work(1)?;
        let (constrained, role) = indexed_role(view, &member.role, input)?;
        if constrained && role.is_none() {
            return Ok(Some(clone_posting(None, execution)?));
        }
        if let Some(things) = indexed_things(&member.thing, input)? {
            let count = things
                .iter()
                .map(|id| {
                    view.thing_candidate_count(*id, role.as_ref(), |set| {
                        pattern.appearances.accepts_roles(set)
                    })
                })
                .sum();
            consider(count, Source::Things(things, role));
        } else if let Some(role) = role {
            let posting = view.role_posting(role.role());
            consider(
                posting.map_or(0, RoaringTreemap::len),
                Source::Role(posting),
            );
        }
    }
    let Some((count, source)) = best else {
        return Ok(None);
    };
    let candidates = match source {
        _ if count == 0 => clone_posting(None, execution)?,
        Source::Role(posting) => clone_posting(posting, execution)?,
        Source::Things(things, role) => posit_posting_for_things(
            view,
            &things,
            role.as_ref(),
            &pattern.appearances,
            execution,
        )?,
        Source::Set(posting) => {
            execution.work(count)?;
            let memory = execution.workspace(|| bitmap_bytes(count))?;
            let mut result = RoaringTreemap::new();
            if let Some(posting) = posting {
                posting.union_into(&mut result);
            }
            BudgetBitmap {
                posting: result,
                memory,
            }
        }
        Source::Posit(id) => BudgetBitmap {
            posting: singleton(id),
            memory: execution.workspace(|| bitmap_bytes(1))?,
        },
    };
    Ok(Some(candidates))
}

fn indexed_role(
    view: &ReadView,
    slot: &RoleSlot,
    input: &Binding,
) -> Result<(bool, Option<Arc<Role>>), DatabaseError> {
    match slot {
        RoleSlot::Wildcard => Ok((false, None)),
        RoleSlot::Named(name) => Ok((true, view.role(name))),
        RoleSlot::Variable(name) => match input.get(name) {
            Some(BoundValue::Role(role)) => Ok((true, Some(Arc::clone(role)))),
            Some(other) => Err(DatabaseError::VariableDomain {
                name: name.clone(),
                first: other.domain().label(),
                second: Domain::Role.label(),
            }),
            None => Ok((false, None)),
        },
    }
}

fn indexed_things(slot: &ThingSlot, input: &Binding) -> Result<Option<Vec<Thing>>, DatabaseError> {
    match slot {
        ThingSlot::Wildcard => Ok(None),
        ThingSlot::Identity(id) => Ok(Some(vec![*id])),
        ThingSlot::Variable(name) | ThingSlot::LegacyVariable(name) => match input.get(name) {
            Some(BoundValue::Thing(identity) | BoundValue::Posit(identity)) => {
                Ok(Some(vec![*identity]))
            }
            Some(other) => Err(DatabaseError::VariableDomain {
                name: name.clone(),
                first: other.domain().label(),
                second: Domain::Thing.label(),
            }),
            None => Ok(None),
        },
        ThingSlot::OneOf(names) => {
            let mut things = Vec::with_capacity(names.len());
            for name in names {
                match input.get(name) {
                    Some(BoundValue::Thing(identity) | BoundValue::Posit(identity)) => {
                        things.push(*identity);
                    }
                    Some(other) => {
                        return Err(DatabaseError::VariableDomain {
                            name: name.clone(),
                            first: other.domain().label(),
                            second: Domain::Thing.label(),
                        });
                    }
                    None => return Err(DatabaseError::InvalidRecall(name.clone())),
                }
            }
            Ok(Some(things))
        }
    }
}

fn posit_posting_for_things(
    view: &ReadView,
    things: &[Thing],
    role: Option<&Arc<Role>>,
    pattern: &AppearanceSetPattern,
    execution: &ExecutionContext,
) -> Result<BudgetBitmap, DatabaseError> {
    let mut appearances: HashSet<_, OtherHasher> = HashSet::default();
    let mut appearance_memory = execution.workspace(|| 0)?;
    for thing in things {
        if let Some(found) = view.appearances(*thing) {
            for appearance in found.iter() {
                execution.work(1)?;
                if role.is_none_or(|role| appearance.role().role() == role.role())
                    && !appearances.contains(appearance)
                {
                    appearance_memory.grow(96)?;
                    appearances.insert(Arc::clone(appearance));
                }
            }
        }
    }

    let mut appearance_sets: HashSet<_, OtherHasher> = HashSet::default();
    let mut set_memory = execution.workspace(|| 0)?;
    for appearance in appearances {
        for set in view.matching_appearance_sets(&appearance, |set| pattern.accepts_roles(set)) {
            execution.work(1)?;
            if !appearance_sets.contains(set) {
                set_memory.grow(96)?;
                appearance_sets.insert(Arc::clone(set));
            }
        }
    }

    let mut posting = RoaringTreemap::new();
    let mut memory = execution.workspace(|| 0)?;
    for appearance_set in appearance_sets {
        if let Some(found) = view.set_posting(&appearance_set) {
            let combined = posting.len().saturating_add(found.len());
            execution.work(found.len())?;
            memory.resize(bitmap_bytes(combined))?;
            found.union_into(&mut posting);
        }
    }
    Ok(BudgetBitmap { posting, memory })
}

fn singleton(identity: Thing) -> RoaringTreemap {
    let mut identities = RoaringTreemap::new();
    identities.insert(identity);
    identities
}

fn match_appearance_set(
    pattern: &AppearanceSetPattern,
    stored: &Arc<AppearanceSet>,
    input: &Binding,
    _domains: &HashMap<String, Domain>,
    execution: &ExecutionContext,
) -> Result<BudgetVec<Binding>, DatabaseError> {
    let mut matches = BudgetVec::new(&execution.budget);
    visit_appearance_set(pattern, stored, input, execution, &mut |binding| {
        matches.push(binding)?;
        Ok(SinkFlow::Continue)
    })?;
    Ok(matches)
}

fn visit_appearance_set(
    pattern: &AppearanceSetPattern,
    stored: &Arc<AppearanceSet>,
    input: &Binding,
    execution: &ExecutionContext,
    visitor: &mut dyn FnMut(Binding) -> Result<SinkFlow, DatabaseError>,
) -> Result<SinkFlow, DatabaseError> {
    execution.work(1)?;
    let _binding = execution.workspace(|| input.heap_bytes().saturating_add(128))?;
    let mut binding = input.clone();
    if let Some(name) = &pattern.binding
        && !binding.bind(name, BoundValue::AppearanceSet(Arc::clone(stored)))?
    {
        return Ok(SinkFlow::Continue);
    }
    if pattern.any {
        return visitor(binding);
    }
    if !pattern.open && pattern.members.len() != stored.appearances().len() {
        return Ok(SinkFlow::Continue);
    }
    if pattern.members.len() > stored.appearances().len() {
        return Ok(SinkFlow::Continue);
    }
    let mut unique: HashSet<Binding, OtherHasher> = HashSet::default();
    let mut unique_memory = execution.workspace(|| 0)?;
    let _used = execution.workspace(|| stored.appearances().len() as u64)?;
    match_members(
        &pattern.members,
        stored.appearances(),
        0,
        &mut vec![false; stored.appearances().len()],
        binding,
        &mut |binding| {
            if stored.appearances().len() > 1 {
                if unique.contains(&binding) {
                    return Ok(SinkFlow::Continue);
                }
                unique_memory.grow(binding.heap_bytes().saturating_add(96))?;
                unique.insert(binding.clone());
            }
            visitor(binding)
        },
        execution,
    )
}

fn match_members(
    patterns: &[AppearancePattern],
    stored: &[Arc<Appearance>],
    index: usize,
    used: &mut [bool],
    binding: Binding,
    visitor: &mut dyn FnMut(Binding) -> Result<SinkFlow, DatabaseError>,
    execution: &ExecutionContext,
) -> Result<SinkFlow, DatabaseError> {
    execution.work(1)?;
    let _binding = execution.workspace(|| binding.heap_bytes().saturating_add(128))?;
    if index == patterns.len() {
        return visitor(binding);
    }
    for (stored_index, appearance) in stored.iter().enumerate() {
        execution.work(1)?;
        if used[stored_index] {
            continue;
        }
        if let Some(next) = match_appearance(&patterns[index], appearance, &binding)? {
            used[stored_index] = true;
            let flow = match_members(patterns, stored, index + 1, used, next, visitor, execution)?;
            used[stored_index] = false;
            if let SinkFlow::Stop = flow {
                return Ok(SinkFlow::Stop);
            }
        }
    }
    Ok(SinkFlow::Continue)
}

fn match_appearance(
    pattern: &AppearancePattern,
    stored: &Arc<Appearance>,
    input: &Binding,
) -> Result<Option<Binding>, DatabaseError> {
    let mut binding = input.clone();
    let thing_matches = match &pattern.thing {
        ThingSlot::Wildcard => true,
        ThingSlot::Identity(id) => stored.thing() == *id,
        ThingSlot::Variable(name) | ThingSlot::LegacyVariable(name) => {
            binding.bind(name, BoundValue::Thing(stored.thing()))?
        }
        ThingSlot::OneOf(names) => {
            let mut matches = false;
            for name in names {
                matches |= match binding.get(name) {
                    Some(BoundValue::Thing(identity) | BoundValue::Posit(identity)) => {
                        *identity == stored.thing()
                    }
                    Some(other) => {
                        return Err(DatabaseError::VariableDomain {
                            name: name.clone(),
                            first: other.domain().label(),
                            second: Domain::Thing.label(),
                        });
                    }
                    None => return Err(DatabaseError::InvalidRecall(name.clone())),
                };
            }
            matches
        }
    };
    if !thing_matches {
        return Ok(None);
    }
    let stored_role = stored.role();
    let role_matches = match &pattern.role {
        RoleSlot::Wildcard => true,
        RoleSlot::Named(name) => stored_role.name() == name,
        RoleSlot::Variable(name) => binding.bind(name, BoundValue::Role(stored_role))?,
    };
    Ok(role_matches.then_some(binding))
}

fn match_fields(
    pattern: &PositPattern,
    posit: &Arc<Posit<LiteralValue>>,
    mut binding: Binding,
) -> Result<Option<Binding>, DatabaseError> {
    if let Some(name) = &pattern.posit
        && !binding.bind(name, BoundValue::Posit(posit.posit()))?
    {
        return Ok(None);
    }
    let value_matches = match &pattern.value {
        ValueSlot::Wildcard => true,
        ValueSlot::Variable(name) | ValueSlot::LegacyVariable(name) => {
            binding.bind(name, BoundValue::Literal(posit.value().clone()))?
        }
        ValueSlot::Literal(literal) => posit
            .value()
            .nominally_equals(literal)
            .map_err(DatabaseError::Comparison)?,
    };
    if !value_matches {
        return Ok(None);
    }
    let time_matches = match &pattern.time {
        TimeSlot::Wildcard => true,
        TimeSlot::Variable(name) | TimeSlot::LegacyVariable(name) => {
            binding.bind(name, BoundValue::Time(posit.time().clone()))?
        }
        TimeSlot::Literal(time) => posit.time() == time,
    };
    Ok(time_matches.then_some(binding))
}

fn maximal(
    matches: Vec<StructuralMatch>,
    mut check: impl FnMut() -> Result<(), DatabaseError>,
) -> Result<Vec<StructuralMatch>, DatabaseError> {
    crate::datatype::temporal::retain_maximal_by_key(
        matches,
        |candidate| candidate.posit.appearance_set(),
        |candidate| candidate.posit.time(),
        |_| check(),
    )
}

fn reduce_matches(
    rows: BudgetVec<StructuralMatch>,
    execution: &ExecutionContext,
) -> Result<BudgetVec<StructuralMatch>, DatabaseError> {
    // Worst case: every row owns a group, frontier, membership slot, and output
    // slot. Store objects referenced by group keys remain shared.
    let _scratch = execution.workspace(|| (rows.len() as u64).saturating_mul(1024))?;
    let (rows, _input) = rows.into_parts();
    let reduced = maximal(rows, || execution.work(1))?;
    BudgetVec::from_vec(reduced, &execution.budget)
}

fn block_has_match(
    plan: &plan::PlannedBranch,
    outer: &Binding,
    view: &ReadView,
    domains: &HashMap<String, Domain>,
    execution: &ExecutionContext,
    effect_cache: &mut EffectCache,
) -> Result<bool, DatabaseError> {
    let mut bindings = BudgetVec::new(&execution.budget);
    let mut input = outer.clone();
    for (name, value) in &plan.anchors.0 {
        if !input.bind(name, value.clone())? {
            return Ok(false);
        }
    }
    bindings.push(input)?;
    for pattern in &plan.patterns {
        let mut joined = BudgetVec::new(&execution.budget);
        for binding in &bindings {
            execution.work(1)?;
            joined.extend(evaluate_pattern(
                view,
                pattern,
                binding,
                domains,
                execution,
                effect_cache,
            )?)?;
        }
        bindings = joined;
        if bindings.is_empty() {
            return Ok(false);
        }
    }
    if plan.filters.is_empty() {
        return Ok(!bindings.is_empty());
    }
    for predicate in &plan.filters {
        let mut kept = BudgetVec::new(&execution.budget);
        for binding in bindings {
            execution.work(1)?;
            if evaluate_predicate(predicate, &binding, execution)? {
                kept.push(binding)?;
            }
        }
        bindings = kept;
    }
    Ok(!bindings.is_empty())
}

fn validate_order_values(
    projected: &[(Binding, Vec<ResultCell>)],
    order: &[OrderItem],
) -> Result<(), DatabaseError> {
    for item in order {
        let mut first_literal: Option<&LiteralValue> = None;
        for (binding, _) in projected {
            let value = binding
                .get(&item.variable)
                .ok_or_else(|| DatabaseError::UnknownVariable(item.variable.clone()))?;
            if let BoundValue::Literal(literal) = value {
                validate_order_literal(literal, first_literal)?;
                if first_literal.is_none() {
                    first_literal = Some(literal);
                }
            }
        }
    }
    Ok(())
}

fn validate_order_literal(
    literal: &LiteralValue,
    first: Option<&LiteralValue>,
) -> Result<(), DatabaseError> {
    if !matches!(
        literal.family(),
        LiteralFamily::Integer | LiteralFamily::Decimal | LiteralFamily::Certainty
    ) {
        return Err(DatabaseError::Comparison(format!(
            "ORDER BY is unsupported for {:?} literals",
            literal.family()
        )));
    }
    if let Some(first) = first {
        first
            .semantic_cmp(literal)
            .map_err(DatabaseError::Comparison)?;
    }
    Ok(())
}

fn compare_order(left: &Binding, right: &Binding, order: &[OrderItem]) -> std::cmp::Ordering {
    for item in order {
        let Some(left) = left.get(&item.variable) else {
            continue;
        };
        let Some(right) = right.get(&item.variable) else {
            continue;
        };
        let mut comparison = order_value(left, right);
        if item.descending {
            comparison = comparison.reverse();
        }
        if comparison != std::cmp::Ordering::Equal {
            return comparison;
        }
    }
    std::cmp::Ordering::Equal
}

fn order_value(left: &BoundValue, right: &BoundValue) -> std::cmp::Ordering {
    match (left, right) {
        (
            BoundValue::Thing(left) | BoundValue::Posit(left),
            BoundValue::Thing(right) | BoundValue::Posit(right),
        ) => left.cmp(right),
        (BoundValue::Role(left), BoundValue::Role(right)) => left.role().cmp(&right.role()),
        (BoundValue::AppearanceSet(left), BoundValue::AppearanceSet(right)) => left.cmp(right),
        (BoundValue::Literal(left), BoundValue::Literal(right)) => left
            .semantic_cmp(right)
            .expect("literal ordering was validated before sorting"),
        (BoundValue::Time(left), BoundValue::Time(right)) => left.cmp(right),
        _ => std::cmp::Ordering::Equal,
    }
}

fn evaluate_predicate(
    predicate: &Predicate,
    binding: &Binding,
    execution: &ExecutionContext,
) -> Result<bool, DatabaseError> {
    let (left, operator, right) = match predicate {
        Predicate::Json(test) => return test.evaluate(binding, execution),
        Predicate::Calendar(test) => {
            return test.evaluate(binding, &execution.metadata.resolved_now);
        }
        Predicate::Comparison {
            left,
            operator,
            right,
        } => (left, operator, right),
    };
    let left = binding
        .get(left)
        .ok_or_else(|| DatabaseError::UnknownVariable(left.clone()))?;
    let owned_right;
    let right = match right {
        Operand::Variable(name) => binding
            .get(name)
            .ok_or_else(|| DatabaseError::UnknownVariable(name.clone()))?,
        Operand::Identity(value) => value,
        Operand::Parameter(name) => {
            return Err(DatabaseError::Parameter(format!(
                "unbound parameter {name}"
            )));
        }
        Operand::Literal(literal) => {
            owned_right = BoundValue::Literal(literal.clone());
            &owned_right
        }
        Operand::Time(time) => {
            owned_right = BoundValue::Time(time.clone());
            &owned_right
        }
    };
    compare(left, operator, right)
}

fn compare(left: &BoundValue, operator: &str, right: &BoundValue) -> Result<bool, DatabaseError> {
    match (left, right) {
        (BoundValue::Literal(left), BoundValue::Literal(right)) => {
            compare_literals(left, operator, right)
        }
        (BoundValue::Time(left), BoundValue::Time(right)) => Ok(match operator {
            "===" | "=" => left == right,
            "?=" => left.overlaps(right),
            "<" => left.definitely_before(right),
            "<=" => left.definitely_at_or_before(right),
            ">" => left.definitely_after(right),
            ">=" => left.definitely_at_or_after(right),
            _ => {
                return Err(DatabaseError::Comparison(format!(
                    "unknown operator '{operator}'"
                )));
            }
        }),
        (
            BoundValue::Thing(left) | BoundValue::Posit(left),
            BoundValue::Thing(right) | BoundValue::Posit(right),
        ) => identity_comparison(*left, operator, *right),
        (BoundValue::Role(left), BoundValue::Role(right)) => {
            identity_comparison(left.role(), operator, right.role())
        }
        (BoundValue::AppearanceSet(left), BoundValue::AppearanceSet(right)) => match operator {
            "===" | "=" | "?=" => Ok(left == right),
            _ => Err(DatabaseError::Comparison(
                "appearance sets support equality only".into(),
            )),
        },
        _ => Err(DatabaseError::Comparison(format!(
            "{} {operator} {} is unsupported",
            left.domain().label(),
            right.domain().label()
        ))),
    }
}

fn compare_literals(
    left: &LiteralValue,
    operator: &str,
    right: &LiteralValue,
) -> Result<bool, DatabaseError> {
    let result = match operator {
        "===" => Ok(left.exactly_equals(right)),
        "=" => left.nominally_equals(right),
        "?=" => left.possibly_equals(right),
        "<" | "<=" | ">" | ">=" => left
            .semantic_cmp(right)
            .map(|ordering| ordering_matches(ordering, operator))
            .map_err(|error| {
                let guidance = if left.family() == LiteralFamily::Certainty
                    || right.family() == LiteralFamily::Certainty
                {
                    "; certainty comparisons require a percent sign (%) on both sides"
                } else {
                    ""
                };
                format!("Ordering comparison not allowed: {error}{guidance}")
            }),
        _ => Err(format!("unknown operator '{operator}'")),
    };
    result.map_err(DatabaseError::Comparison)
}

fn identity_comparison(left: Thing, operator: &str, right: Thing) -> Result<bool, DatabaseError> {
    match operator {
        "===" | "=" | "?=" => Ok(left == right),
        _ => Err(DatabaseError::Comparison(
            "identity values support equality only".into(),
        )),
    }
}

fn ordering_matches(ordering: std::cmp::Ordering, operator: &str) -> bool {
    use std::cmp::Ordering::{Equal, Greater, Less};
    matches!(
        (ordering, operator),
        (Less, "<" | "<=") | (Equal, "<=" | ">=") | (Greater, ">" | ">=")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::construct::PersistenceMode;

    #[test]
    fn snapshot_reduction_checks_cancellation_during_grouping() {
        let posit = Arc::new(Posit::new(
            10,
            Arc::new(AppearanceSet::new(Vec::new()).unwrap()),
            LiteralValue::new("1", LiteralFamily::Integer).unwrap(),
            Time::new_year_from("2024").unwrap(),
        ));
        let candidates = vec![
            StructuralMatch {
                posit,
                binding: Binding::default()
            };
            10
        ];
        let mut checks = 0;
        let result = maximal(candidates, || {
            checks += 1;
            if checks == 4 {
                Err(DatabaseError::Cancelled)
            } else {
                Ok(())
            }
        });
        assert!(matches!(result, Err(DatabaseError::Cancelled)));
        assert_eq!(checks, 4);
    }

    fn pattern(thing: ThingSlot, role: RoleSlot) -> PositPattern {
        PositPattern {
            posit: None,
            appearances: AppearanceSetPattern {
                binding: None,
                members: vec![AppearancePattern { thing, role }],
                open: false,
                any: false,
            },
            value: ValueSlot::Wildcard,
            time: TimeSlot::Wildcard,
            cutoff: None,
            latest_matching: false,
            effect: None,
        }
    }

    #[test]
    fn candidate_planning_uses_role_and_bound_thing_indexes() {
        let database = Database::new(PersistenceMode::InMemory).unwrap();
        let engine = Engine::new(&database);
        engine
            .execute(
                "add role common, rare; add posit \
                 [{(+a, common)}, 1, @NOW], [{(a, rare)}, 1, @NOW], \
                 [{(+b, common)}, 2, @NOW], [{(+c, common)}, 3, @NOW], \
                 [{(c, rare)}, 1, @NOW];",
            )
            .unwrap();

        let view = database.read_view();
        let rare = pattern(
            ThingSlot::Variable("entity".to_string()),
            RoleSlot::Named("rare".to_string()),
        );
        let rare_candidates = indexed_candidate_identities(
            &view,
            &rare,
            &Binding::default(),
            &ExecutionOptions::default().resolve("").unwrap(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(rare_candidates.len(), 2);

        let pattern_order = [
            pattern(
                ThingSlot::Variable("entity".to_string()),
                RoleSlot::Named("common".to_string()),
            ),
            rare.clone(),
        ];
        let planned =
            plan_patterns(&view, &pattern_order, &HashSet::new(), &Binding::default()).unwrap();
        assert!(matches!(
            &planned[0].appearances.members[0].role,
            RoleSlot::Named(name) if name == "rare"
        ));

        let first = engine
            .execute_collect(
                "search [{(?entity, rare)}, *, *] return ?entity order by ?entity limit 1;",
            )
            .unwrap();
        let identity = first.rows[0][0].as_str().parse::<Thing>().unwrap();
        let mut binding = Binding::default();
        assert!(binding.bind("entity", BoundValue::Thing(identity)).unwrap());
        let common = pattern(
            ThingSlot::Variable("entity".to_string()),
            RoleSlot::Named("common".to_string()),
        );
        let bound_candidates = indexed_candidate_identities(
            &view,
            &common,
            &binding,
            &ExecutionOptions::default().resolve("").unwrap(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(bound_candidates.len(), 1);

        let unconstrained = PositPattern {
            posit: None,
            appearances: AppearanceSetPattern {
                binding: None,
                members: Vec::new(),
                open: false,
                any: true,
            },
            value: ValueSlot::Wildcard,
            time: TimeSlot::Wildcard,
            cutoff: None,
            latest_matching: false,
            effect: None,
        };
        assert!(
            indexed_candidate_identities(
                &view,
                &unconstrained,
                &Binding::default(),
                &ExecutionOptions::default().resolve("").unwrap()
            )
            .unwrap()
            .is_none()
        );
    }
}
