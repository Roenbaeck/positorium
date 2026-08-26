use super::*;
use crate::construct::{Appearance, AppearanceSet, Posit, Role};
use pest::iterators::Pair;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Domain {
    Thing,
    Role,
    Posit,
    AppearanceSet,
    Literal,
    Time,
}

impl Domain {
    fn label(self) -> &'static str {
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

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Binding(BTreeMap<String, BoundValue>);

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
            (BoundValue::Thing(left), BoundValue::Posit(right)) if left == right => {
                self.0.insert(name.to_string(), value);
                Ok(true)
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
    Variable(String),
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

#[derive(Debug, Clone)]
enum ValueSlot {
    Wildcard,
    Variable(String),
    Literal(LiteralValue),
}

#[derive(Debug, Clone)]
enum TimeSlot {
    Wildcard,
    Variable(String),
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
}

#[derive(Debug, Clone)]
enum Operand {
    Variable(String),
    Literal(LiteralValue),
    Time(Time),
}

#[derive(Debug, Clone)]
struct Predicate {
    left: String,
    operator: String,
    right: Operand,
}

#[derive(Debug)]
struct Query {
    patterns: Vec<PositPattern>,
    predicates: Vec<Predicate>,
    returns: Vec<String>,
}

#[derive(Clone)]
struct StructuralMatch {
    posit: Arc<Posit<LiteralValue>>,
    binding: Binding,
}

pub(super) fn execute(
    database: &Database,
    command: Pair<'_, Rule>,
    sink: &mut dyn RowSink,
    return_columns: &mut Option<Vec<String>>,
    execution: &ExecutionContext,
) -> Result<(), DatabaseError> {
    execution.check()?;
    let query = parse_query(command, &execution.metadata.resolved_now)?;
    let domains = validate_domains(&query)?;
    let patterns = plan_patterns(&query.patterns)?;
    let posits = snapshot(database)?;

    let mut bindings = vec![Binding::default()];
    for pattern in patterns {
        let mut joined = Vec::new();
        for binding in &bindings {
            execution.check()?;
            joined.extend(evaluate_pattern(
                pattern, binding, &posits, &domains, execution,
            )?);
        }
        bindings = joined;
        if bindings.is_empty() {
            break;
        }
    }

    for predicate in &query.predicates {
        let mut selected = Vec::with_capacity(bindings.len());
        for binding in bindings {
            execution.check()?;
            if evaluate_predicate(predicate, &binding)? {
                selected.push(binding);
            }
        }
        bindings = selected;
    }

    let columns = query.returns.clone();
    *return_columns = Some(columns.clone());
    if let SinkFlow::Stop = sink.on_meta(&columns) {
        return Ok(());
    }
    for binding in bindings {
        execution.check()?;
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
        if let SinkFlow::Stop = sink.push(row) {
            break;
        }
    }
    Ok(())
}

fn snapshot(database: &Database) -> Result<Vec<Arc<Posit<LiteralValue>>>, DatabaseError> {
    let reverse = database.posit_thing_to_appearance_set_lookup();
    let mut identities: Vec<_> = reverse
        .lock()
        .map_err(|error| DatabaseError::Lock(error.to_string()))?
        .keys()
        .copied()
        .collect();
    identities.sort_unstable();
    let keeper = database.posit_keeper();
    let mut keeper = keeper
        .lock()
        .map_err(|error| DatabaseError::Lock(error.to_string()))?;
    identities
        .into_iter()
        .map(|identity| {
            keeper.posit::<LiteralValue>(identity).ok_or_else(|| {
                DatabaseError::Invariant(format!(
                    "posit index contains identity {identity} without a literal posit"
                ))
            })
        })
        .collect()
}

fn parse_query(command: Pair<'_, Rule>, resolved_now: &Time) -> Result<Query, DatabaseError> {
    let mut patterns = Vec::new();
    let mut predicates = Vec::new();
    let mut returns = Vec::new();
    for clause in command.into_inner() {
        match clause.as_rule() {
            Rule::search_clause => {
                for pattern in clause.into_inner() {
                    if pattern.as_rule() == Rule::posit_search {
                        patterns.push(parse_pattern(pattern, resolved_now)?);
                    }
                }
            }
            Rule::where_clause => {
                for condition in clause.into_inner() {
                    if condition.as_rule() == Rule::condition {
                        predicates.push(parse_predicate(condition, resolved_now)?);
                    }
                }
            }
            Rule::return_clause => {
                for variable in clause.into_inner() {
                    if matches!(variable.as_rule(), Rule::recall | Rule::insert) {
                        returns.push(variable_name(&variable));
                    }
                }
            }
            Rule::limit_clause => {}
            rule => {
                return Err(DatabaseError::Invariant(format!(
                    "unexpected query clause {rule:?}"
                )));
            }
        }
    }
    if patterns.is_empty() {
        return Err(DatabaseError::Execution(
            "a search requires at least one posit pattern".into(),
        ));
    }
    if returns.is_empty() {
        return Err(DatabaseError::Execution(
            "a search requires at least one return variable".into(),
        ));
    }
    Ok(Query {
        patterns,
        predicates,
        returns,
    })
}

fn parse_pattern(
    pattern: Pair<'_, Rule>,
    resolved_now: &Time,
) -> Result<PositPattern, DatabaseError> {
    let mut posit = None;
    let mut appearances = None;
    let mut value = None;
    let mut time = None;
    let mut cutoff = None;
    for component in pattern.into_inner() {
        match component.as_rule() {
            Rule::insert | Rule::recall if appearances.is_none() => {
                posit = Some(variable_name(&component));
            }
            Rule::appearance_set_search => {
                appearances = Some(parse_appearance_set(component)?);
            }
            Rule::appearing_value_search => {
                value = Some(parse_value_slot(component, resolved_now)?);
            }
            Rule::appearance_time_search => {
                time = Some(parse_time_slot(component, resolved_now)?);
            }
            Rule::as_of_clause => {
                let component = component
                    .into_inner()
                    .next()
                    .ok_or_else(|| DatabaseError::Invariant("as-of clause has no cutoff".into()))?;
                cutoff = Some(match component.as_rule() {
                    Rule::recall | Rule::insert => Cutoff::Variable(variable_name(&component)),
                    Rule::constant | Rule::time => Cutoff::Literal(
                        parse_time_with_now(component.as_str(), resolved_now).ok_or_else(|| {
                            DatabaseError::Execution(format!(
                                "invalid as-of cutoff '{}'",
                                component.as_str()
                            ))
                        })?,
                    ),
                    rule => {
                        return Err(DatabaseError::Invariant(format!(
                            "unexpected as-of cutoff {rule:?}"
                        )));
                    }
                });
            }
            rule => {
                return Err(DatabaseError::Invariant(format!(
                    "unexpected posit-pattern component {rule:?}"
                )));
            }
        }
    }
    Ok(PositPattern {
        posit,
        appearances: appearances.ok_or_else(|| {
            DatabaseError::Invariant("posit pattern has no appearance-set slot".into())
        })?,
        value: value
            .ok_or_else(|| DatabaseError::Invariant("posit pattern has no value slot".into()))?,
        time: time
            .ok_or_else(|| DatabaseError::Invariant("posit pattern has no time slot".into()))?,
        cutoff,
        latest_matching: false,
    })
}

fn parse_appearance_set(pattern: Pair<'_, Rule>) -> Result<AppearanceSetPattern, DatabaseError> {
    let mut members = Vec::new();
    let mut any = false;
    for member in pattern.into_inner() {
        match member.as_rule() {
            Rule::wildcard => any = true,
            Rule::appearance_search => members.push(parse_appearance(member)?),
            rule => {
                return Err(DatabaseError::Invariant(format!(
                    "unexpected appearance-set member {rule:?}"
                )));
            }
        }
    }
    Ok(AppearanceSetPattern {
        binding: None,
        members,
        open: true,
        any,
    })
}

fn parse_appearance(appearance: Pair<'_, Rule>) -> Result<AppearancePattern, DatabaseError> {
    let mut inner = appearance.into_inner();
    let thing = inner
        .next()
        .ok_or_else(|| DatabaseError::Invariant("appearance lacks a Thing slot".into()))?;
    let role = inner
        .next()
        .ok_or_else(|| DatabaseError::Invariant("appearance lacks a Role slot".into()))?;
    let thing = match thing.as_rule() {
        Rule::wildcard => ThingSlot::Wildcard,
        Rule::insert | Rule::recall => ThingSlot::Variable(variable_name(&thing)),
        Rule::recall_union => ThingSlot::OneOf(
            thing
                .into_inner()
                .filter(|part| part.as_rule() == Rule::recall)
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
        Rule::insert | Rule::recall => RoleSlot::Variable(variable_name(&role)),
        rule => {
            return Err(DatabaseError::Invariant(format!(
                "unexpected Role slot {rule:?}"
            )));
        }
    };
    Ok(AppearancePattern { thing, role })
}

fn parse_value_slot(slot: Pair<'_, Rule>, resolved_now: &Time) -> Result<ValueSlot, DatabaseError> {
    let value = slot
        .into_inner()
        .next()
        .ok_or_else(|| DatabaseError::Invariant("value pattern is empty".into()))?;
    match value.as_rule() {
        Rule::wildcard => Ok(ValueSlot::Wildcard),
        Rule::insert | Rule::recall => Ok(ValueSlot::Variable(variable_name(&value))),
        rule => parse_lossless_literal(rule, value.as_str(), resolved_now).map(ValueSlot::Literal),
    }
}

fn parse_time_slot(slot: Pair<'_, Rule>, resolved_now: &Time) -> Result<TimeSlot, DatabaseError> {
    let time = slot
        .into_inner()
        .next()
        .ok_or_else(|| DatabaseError::Invariant("time pattern is empty".into()))?;
    match time.as_rule() {
        Rule::wildcard => Ok(TimeSlot::Wildcard),
        Rule::insert | Rule::recall => Ok(TimeSlot::Variable(variable_name(&time))),
        Rule::constant | Rule::time => parse_time_with_now(time.as_str(), resolved_now)
            .map(TimeSlot::Literal)
            .ok_or_else(|| {
                DatabaseError::Execution(format!("invalid time literal '{}'", time.as_str()))
            }),
        rule => Err(DatabaseError::Invariant(format!(
            "unexpected time slot {rule:?}"
        ))),
    }
}

fn parse_predicate(
    condition: Pair<'_, Rule>,
    resolved_now: &Time,
) -> Result<Predicate, DatabaseError> {
    let mut parts = condition.into_inner();
    let left = parts
        .next()
        .ok_or_else(|| DatabaseError::Invariant("predicate has no left operand".into()))?;
    let operator = parts
        .next()
        .ok_or_else(|| DatabaseError::Invariant("predicate has no operator".into()))?;
    let right = parts
        .next()
        .ok_or_else(|| DatabaseError::Invariant("predicate has no right operand".into()))?;
    Ok(Predicate {
        left: variable_name(&left),
        operator: operator.as_str().to_string(),
        right: parse_operand(right, resolved_now)?,
    })
}

fn parse_operand(operand: Pair<'_, Rule>, resolved_now: &Time) -> Result<Operand, DatabaseError> {
    if operand.as_rule() == Rule::rhs_value {
        return parse_operand(
            operand
                .into_inner()
                .next()
                .ok_or_else(|| DatabaseError::Invariant("empty right operand".into()))?,
            resolved_now,
        );
    }
    match operand.as_rule() {
        Rule::recall | Rule::insert => Ok(Operand::Variable(variable_name(&operand))),
        Rule::constant | Rule::time => parse_time_with_now(operand.as_str(), resolved_now)
            .map(Operand::Time)
            .ok_or_else(|| {
                DatabaseError::Execution(format!("invalid time literal '{}'", operand.as_str()))
            }),
        rule => parse_lossless_literal(rule, operand.as_str().trim(), resolved_now)
            .map(Operand::Literal),
    }
}

fn variable_name(variable: &Pair<'_, Rule>) -> String {
    variable
        .as_str()
        .trim()
        .trim_start_matches(['+', '?'])
        .to_string()
}

fn canonical_role_name(token: &str) -> String {
    token.trim().nfc().collect()
}

fn validate_domains(query: &Query) -> Result<HashMap<String, Domain>, DatabaseError> {
    let mut domains = HashMap::new();
    for pattern in &query.patterns {
        register_pattern(pattern, &mut domains)?;
    }
    for pattern in &query.patterns {
        for dependency in pattern.dependencies() {
            if !domains.contains_key(dependency) {
                return Err(DatabaseError::InvalidRecall(format!(
                    "'{dependency}' is not bound by a positive pattern"
                )));
            }
        }
    }
    for predicate in &query.predicates {
        require_variable(&domains, &predicate.left)?;
        if let Operand::Variable(name) = &predicate.right {
            require_variable(&domains, name)?;
        }
    }
    for returned in &query.returns {
        require_variable(&domains, returned)?;
    }
    Ok(domains)
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
        if let ThingSlot::Variable(name) = &member.thing {
            register_domain(domains, name, Domain::Thing)?;
        }
        if let RoleSlot::Variable(name) = &member.role {
            register_domain(domains, name, Domain::Role)?;
        }
    }
    if let ValueSlot::Variable(name) = &pattern.value {
        register_domain(domains, name, Domain::Literal)?;
    }
    if let TimeSlot::Variable(name) = &pattern.time {
        register_domain(domains, name, Domain::Time)?;
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
            if let ThingSlot::Variable(name) = &member.thing {
                names.insert(name.as_str());
            }
            if let RoleSlot::Variable(name) = &member.role {
                names.insert(name.as_str());
            }
        }
        if let ValueSlot::Variable(name) = &self.value {
            names.insert(name.as_str());
        }
        if let TimeSlot::Variable(name) = &self.time {
            names.insert(name.as_str());
        }
        names
    }

    fn dependencies(&self) -> HashSet<&str> {
        let mut names = HashSet::new();
        if let Some(Cutoff::Variable(name)) = &self.cutoff {
            names.insert(name.as_str());
        }
        for member in &self.appearances.members {
            if let ThingSlot::OneOf(union) = &member.thing {
                names.extend(union.iter().map(String::as_str));
            }
        }
        names
    }
}

fn plan_patterns(patterns: &[PositPattern]) -> Result<Vec<&PositPattern>, DatabaseError> {
    let mut pending: Vec<_> = patterns.iter().collect();
    let mut planned = Vec::with_capacity(patterns.len());
    let mut available = HashSet::new();
    while !pending.is_empty() {
        let Some(index) = pending
            .iter()
            .position(|pattern| pattern.dependencies().is_subset(&available))
        else {
            let dependencies: Vec<_> = pending
                .iter()
                .flat_map(|pattern| pattern.dependencies())
                .filter(|name| !available.contains(name))
                .collect();
            return Err(DatabaseError::InvalidRecall(format!(
                "query has unresolvable variable dependencies: {}",
                dependencies.join(", ")
            )));
        };
        let pattern = pending.remove(index);
        available.extend(pattern.introduced());
        planned.push(pattern);
    }
    Ok(planned)
}

fn evaluate_pattern(
    pattern: &PositPattern,
    input: &Binding,
    posits: &[Arc<Posit<LiteralValue>>],
    domains: &HashMap<String, Domain>,
    execution: &ExecutionContext,
) -> Result<Vec<Binding>, DatabaseError> {
    let cutoff = match &pattern.cutoff {
        None => None,
        Some(Cutoff::Literal(time)) => Some(time.clone()),
        Some(Cutoff::Variable(name)) => match input.get(name) {
            Some(BoundValue::Time(time)) => Some(time.clone()),
            Some(other) => {
                return Err(DatabaseError::VariableDomain {
                    name: name.clone(),
                    first: other.domain().label(),
                    second: Domain::Time.label(),
                });
            }
            None => return Err(DatabaseError::InvalidRecall(name.clone())),
        },
    };

    let mut structural = Vec::new();
    for posit in posits {
        execution.check()?;
        if cutoff
            .as_ref()
            .is_some_and(|cutoff| !posit.time().definitely_at_or_before(cutoff))
        {
            continue;
        }
        for binding in match_appearance_set(
            &pattern.appearances,
            &posit.appearance_set(),
            input,
            domains,
        )? {
            structural.push(StructuralMatch {
                posit: Arc::clone(posit),
                binding,
            });
        }
    }

    let candidates = if pattern.cutoff.is_some() && !pattern.latest_matching {
        maximal(structural)
    } else {
        structural
    };
    let mut matched = Vec::new();
    for candidate in candidates {
        execution.check()?;
        if let Some(binding) = match_fields(pattern, &candidate.posit, candidate.binding)? {
            matched.push(StructuralMatch {
                posit: candidate.posit,
                binding,
            });
        }
    }
    let matched = if pattern.cutoff.is_some() && pattern.latest_matching {
        maximal(matched)
    } else {
        matched
    };
    Ok(matched.into_iter().map(|matched| matched.binding).collect())
}

fn match_appearance_set(
    pattern: &AppearanceSetPattern,
    stored: &Arc<AppearanceSet>,
    input: &Binding,
    _domains: &HashMap<String, Domain>,
) -> Result<Vec<Binding>, DatabaseError> {
    let mut binding = input.clone();
    if let Some(name) = &pattern.binding
        && !binding.bind(name, BoundValue::AppearanceSet(Arc::clone(stored)))?
    {
        return Ok(Vec::new());
    }
    if pattern.any {
        return Ok(vec![binding]);
    }
    if !pattern.open && pattern.members.len() != stored.appearances().len() {
        return Ok(Vec::new());
    }
    if pattern.members.len() > stored.appearances().len() {
        return Ok(Vec::new());
    }
    let mut matches = Vec::new();
    match_members(
        &pattern.members,
        stored.appearances(),
        0,
        &mut vec![false; stored.appearances().len()],
        binding,
        &mut matches,
    )?;
    let mut unique = Vec::new();
    for binding in matches {
        if !unique.contains(&binding) {
            unique.push(binding);
        }
    }
    Ok(unique)
}

fn match_members(
    patterns: &[AppearancePattern],
    stored: &[Arc<Appearance>],
    index: usize,
    used: &mut [bool],
    binding: Binding,
    matches: &mut Vec<Binding>,
) -> Result<(), DatabaseError> {
    if index == patterns.len() {
        matches.push(binding);
        return Ok(());
    }
    for (stored_index, appearance) in stored.iter().enumerate() {
        if used[stored_index] {
            continue;
        }
        if let Some(next) = match_appearance(&patterns[index], appearance, &binding)? {
            used[stored_index] = true;
            match_members(patterns, stored, index + 1, used, next, matches)?;
            used[stored_index] = false;
        }
    }
    Ok(())
}

fn match_appearance(
    pattern: &AppearancePattern,
    stored: &Arc<Appearance>,
    input: &Binding,
) -> Result<Option<Binding>, DatabaseError> {
    let mut binding = input.clone();
    let thing_matches = match &pattern.thing {
        ThingSlot::Wildcard => true,
        ThingSlot::Variable(name) => binding.bind(name, BoundValue::Thing(stored.thing()))?,
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
        ValueSlot::Variable(name) => {
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
        TimeSlot::Variable(name) => binding.bind(name, BoundValue::Time(posit.time().clone()))?,
        TimeSlot::Literal(time) => posit.time() == time,
    };
    Ok(time_matches.then_some(binding))
}

fn maximal(matches: Vec<StructuralMatch>) -> Vec<StructuralMatch> {
    matches
        .iter()
        .enumerate()
        .filter(|(index, candidate)| {
            !matches.iter().enumerate().any(|(other_index, other)| {
                *index != other_index
                    && candidate.posit.appearance_set() == other.posit.appearance_set()
                    && candidate.posit.time().definitely_before(other.posit.time())
            })
        })
        .map(|(_, candidate)| candidate.clone())
        .collect()
}

fn evaluate_predicate(predicate: &Predicate, binding: &Binding) -> Result<bool, DatabaseError> {
    let left = binding
        .get(&predicate.left)
        .ok_or_else(|| DatabaseError::UnknownVariable(predicate.left.clone()))?;
    let owned_right;
    let right = match &predicate.right {
        Operand::Variable(name) => binding
            .get(name)
            .ok_or_else(|| DatabaseError::UnknownVariable(name.clone()))?,
        Operand::Literal(literal) => {
            owned_right = BoundValue::Literal(literal.clone());
            &owned_right
        }
        Operand::Time(time) => {
            owned_right = BoundValue::Time(time.clone());
            &owned_right
        }
    };
    compare(left, &predicate.operator, right)
}

fn compare(left: &BoundValue, operator: &str, right: &BoundValue) -> Result<bool, DatabaseError> {
    let operator = if operator == "==" { "=" } else { operator };
    match (left, right) {
        (BoundValue::Literal(left), BoundValue::Literal(right)) => {
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
