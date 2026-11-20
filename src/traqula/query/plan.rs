//! Typed physical plans and their versioned, read-only explanation.
//!
//! Planning fixes dependency order and access paths once per search. Execution
//! consumes these choices, including inside correlated anti-joins. Explanations
//! are derived from that plan rather than from a second, descriptive planner.

use super::*;
use serde::Serialize;

pub const QUERY_PLAN_VERSION: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanDomain {
    Thing,
    Role,
    Posit,
    AppearanceSet,
    Literal,
    Time,
}

impl From<Domain> for PlanDomain {
    fn from(domain: Domain) -> Self {
        match domain {
            Domain::Thing => Self::Thing,
            Domain::Role => Self::Role,
            Domain::Posit => Self::Posit,
            Domain::AppearanceSet => Self::AppearanceSet,
            Domain::Literal => Self::Literal,
            Domain::Time => Self::Time,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanBinding {
    pub name: String,
    pub domain: PlanDomain,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanAccess {
    FullScan,
    Bitmap {
        roles: Vec<String>,
        bindings: Vec<String>,
    },
    EffectiveSlice,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum PlanCutoff {
    Literal(String),
    Variable(String),
}

impl From<&Cutoff> for PlanCutoff {
    fn from(cutoff: &Cutoff) -> Self {
        match cutoff {
            Cutoff::Literal(time) => Self::Literal(time.to_string()),
            Cutoff::Variable(name) => Self::Variable(name.clone()),
        }
    }
}

/// Physical operators, including blocking reductions and bounded ordering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "operator", rename_all = "snake_case")]
pub enum PlanOperator {
    StructuralScan {
        source_pattern: usize,
        access: PlanAccess,
        estimated_candidates: u64,
        exact: bool,
        appearances: String,
        dependencies: Vec<String>,
    },
    MatchFields {
        posit: Option<String>,
        value: String,
        time: String,
    },
    MaximalByAppearanceSet {
        cutoff: PlanCutoff,
    },
    ResolveEffect {
        assertion_cutoff: PlanCutoff,
        appearance_cutoff: PlanCutoff,
        estimated_assertion_candidates: u64,
        dependencies: Vec<String>,
    },
    MatchEffectiveTarget {
        source_pattern: usize,
        target: String,
        via: Option<String>,
    },
    JoinExternalFields {
        variables: Vec<String>,
    },
    NestedLoopJoin {
        scope: String,
        pattern_order: Vec<usize>,
    },
    UnionAll,
    AntiJoin {
        correlated: Vec<String>,
    },
    Filter {
        predicates: Vec<String>,
    },
    Project {
        columns: Vec<String>,
    },
    Distinct,
    Sort {
        keys: Vec<PlanOrder>,
    },
    TopK {
        keys: Vec<PlanOrder>,
        /// Includes one extra row to determine the result's `limited` flag.
        rows: usize,
    },
    Limit {
        rows: usize,
        presentation_only: bool,
    },
    IdentityBindings,
    /// Evaluate a search with no projection and no mutation bindings to promote.
    Consume,
    SearchOrAdd {
        fallback: String,
        allocations: Vec<PlanBinding>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanOrder {
    pub variable: String,
    pub descending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanNode {
    #[serde(flatten)]
    pub operation: PlanOperator,
    pub bindings: Vec<PlanBinding>,
    pub inputs: Vec<PlanNode>,
}

impl PlanNode {
    fn wrap(self, operation: PlanOperator) -> Self {
        Self {
            operation,
            bindings: self.bindings.clone(),
            inputs: vec![self],
        }
    }
}

/// Versioned explanation of one search, with parameters and `@NOW` resolved
/// for this invocation. Candidate estimates are upper bounds from index sizes,
/// not measured row counts or estimates of post-filter cardinality.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QueryPlan {
    pub version: u16,
    pub resolved_now: String,
    /// Whether joins, filters, and projection pass rows incrementally.
    pub incremental: bool,
    /// Why a plan retains the original full-evaluation barriers, if required.
    pub materialization_reason: Option<String>,
    pub root: PlanNode,
}

pub(super) struct PlannedPattern {
    pub pattern: PositPattern,
    pub access: PlanAccess,
    source_pattern: usize,
    estimated_candidates: u64,
    input_domains: HashMap<String, Domain>,
    pub predicates: Vec<Predicate>,
}

pub(super) struct PlannedBranch {
    pub anchors: Binding,
    pub filters: Vec<Predicate>,
    pub patterns: Vec<PlannedPattern>,
    domains: HashMap<String, Domain>,
    scope: String,
}

pub(super) struct PhysicalQuery {
    pub has_fallback: bool,
    pub branches: Vec<PlannedBranch>,
    pub not_exists: Vec<PlannedBranch>,
    pub predicates: Vec<Predicate>,
    pub returns: Vec<String>,
    pub distinct: bool,
    pub order: Vec<OrderItem>,
    pub domains: HashMap<String, Domain>,
    pub limit: Option<usize>,
    pub materialization_reason: Option<String>,
}

pub(super) fn compile(
    view: &ReadView,
    prepared: PreparedQuery,
    execution: &ExecutionContext,
) -> Result<PhysicalQuery, DatabaseError> {
    execution.check()?;
    let PreparedQuery { query, domains } = prepared;
    let materialization_reason = materialization_reason(&query, &domains);
    let mut not_exists = Vec::new();
    let mut absence_checks_are_total = true;
    for (index, block) in query.not_exists.into_iter().enumerate() {
        let mut local_domains = domains.clone();
        for pattern in &block.patterns {
            register_pattern(pattern, &mut local_domains)?;
        }
        let total = total_before_filter(
            view,
            block.patterns.iter(),
            &block.predicates,
            &local_domains,
        );
        absence_checks_are_total &= total;
        let anchors = if total {
            identity_anchors(&block.predicates, &local_domains)
        } else {
            Binding::default()
        };
        let mut plan = compile_branch(
            view,
            block.patterns,
            domains.clone(),
            anchors,
            format!("not_exists_{}", index + 1),
            execution,
        )?;
        plan.filters = block.predicates;
        not_exists.push(plan);
    }
    let mut anchors = identity_anchors(&query.predicates, &domains);
    if !anchors.0.is_empty()
        && (!absence_checks_are_total
            || !total_before_filter(
                view,
                query.branches.iter().flatten(),
                &query.predicates,
                &domains,
            ))
    {
        anchors = Binding::default();
    }
    let mut branches = Vec::new();
    for (index, patterns) in query.branches.into_iter().enumerate() {
        branches.push(compile_branch(
            view,
            patterns,
            HashMap::new(),
            anchors.clone(),
            format!("branch_{}", index + 1),
            execution,
        )?);
    }
    if materialization_reason.is_none() {
        for branch in &mut branches {
            // Predicates are total in this mode. Apply each as soon as both
            // operands exist, always after any temporal reduction in a pattern.
            let mut available = HashSet::new();
            let mut remaining: Vec<_> = query.predicates.iter().collect();
            for pattern in &mut branch.patterns {
                available.extend(pattern.pattern.introduced());
                remaining.retain(|predicate| {
                    let Predicate::Comparison { left, right, .. } = predicate else {
                        unreachable!("calendar operations are validated across all matches")
                    };
                    let ready = available.contains(left.as_str())
                        && match right {
                            Operand::Variable(name) => available.contains(name.as_str()),
                            _ => true,
                        };
                    if ready {
                        pattern.predicates.push((*predicate).clone());
                    }
                    !ready
                });
            }
            debug_assert!(remaining.is_empty());
        }
    }
    Ok(PhysicalQuery {
        has_fallback: query.has_fallback,
        branches,
        not_exists,
        domains,
        predicates: query.predicates,
        returns: query.returns,
        distinct: query.distinct,
        order: query.order,
        limit: execution.effective_limit(query.limit),
        materialization_reason,
    })
}

fn materialization_reason(query: &Query, domains: &HashMap<String, Domain>) -> Option<String> {
    let reason = if query.has_fallback {
        "search_or_add_requires_complete_identity_bindings"
    } else if query.not_exists.iter().any(|b| !b.predicates.is_empty()) {
        "absence_predicates_require_complete_validation"
    } else if query
        .branches
        .iter()
        .chain(query.not_exists.iter().map(|b| &b.patterns))
        .flatten()
        .any(|p| p.effect.is_some())
    {
        "effective_evidence_requires_complete_validation"
    } else if query
        .branches
        .iter()
        .chain(query.not_exists.iter().map(|b| &b.patterns))
        .flatten()
        .any(|p| matches!(p.value, ValueSlot::Literal(_)))
        || query.predicates.iter().any(|p| match p {
            Predicate::Calendar(_) => true,
            Predicate::Comparison { left, operator, .. } => {
                domains[left] == Domain::Literal && operator != "==="
            }
        })
    {
        "literal_comparisons_require_complete_validation"
    } else {
        return None;
    };
    Some(reason.to_string())
}

// Moving a WHERE comparison before candidates, effect resolution or other
// filters must preserve errors the complete evaluation could have reported.
fn total_before_filter<'a>(
    view: &ReadView,
    mut patterns: impl Iterator<Item = &'a PositPattern>,
    predicates: &[Predicate],
    domains: &HashMap<String, Domain>,
) -> bool {
    patterns.all(|pattern| {
        pattern.effect.is_none()
            && match &pattern.value {
                ValueSlot::Literal(value) => literal_comparison_is_total(view, pattern, value),
                _ => true,
            }
    }) && predicates.iter().all(|predicate| {
        matches!(predicate, Predicate::Comparison { left, operator, .. }
        if domains[left] != Domain::Literal || operator == "===")
    })
}

fn identity_anchors(predicates: &[Predicate], domains: &HashMap<String, Domain>) -> Binding {
    let mut anchors = Binding::default();
    for predicate in predicates {
        if let Predicate::Comparison {
            left,
            operator,
            right: Operand::Identity(BoundValue::Thing(id) | BoundValue::Posit(id)),
        } = predicate
            && matches!(operator.as_str(), "=" | "===" | "?=")
        {
            // The query slot controls the projected kind of a matching identity.
            let value = if domains[left] == Domain::Posit {
                BoundValue::Posit(*id)
            } else {
                BoundValue::Thing(*id)
            };
            anchors.0.entry(left.clone()).or_insert(value);
        }
    }
    anchors
}

fn compile_branch(
    view: &ReadView,
    patterns: Vec<PositPattern>,
    mut domains: HashMap<String, Domain>,
    anchors: Binding,
    scope: String,
    execution: &ExecutionContext,
) -> Result<PlannedBranch, DatabaseError> {
    domains.extend(
        anchors
            .0
            .iter()
            .map(|(name, value)| (name.clone(), value.domain())),
    );
    let initially_available = domains.keys().map(String::as_str).collect();
    let ordered = plan_patterns(view, &patterns, &initially_available, &anchors)?;
    let mut planned = Vec::new();
    for pattern in ordered {
        execution.check()?;
        let source_pattern = patterns
            .iter()
            .position(|p| std::ptr::eq(p, pattern))
            .unwrap()
            + 1;
        let available = domains.keys().map(String::as_str).collect();
        let access = access_path(view, pattern, &available);
        let estimated_candidates = if pattern.effect.is_some() {
            [
                crate::construct::POSIT_ROLE_ID,
                crate::construct::ASCERTAINS_ROLE_ID,
            ]
            .iter()
            .map(|role| view.role_posting(*role).map_or(0, RoaringTreemap::len))
            .min()
            .unwrap_or(0)
        } else {
            let estimate = estimated_candidate_count(view, pattern, &anchors);
            if estimate == u64::MAX {
                view.posit_count() as u64
            } else {
                estimate
            }
        };
        planned.push(PlannedPattern {
            pattern: pattern.clone(),
            access,
            source_pattern,
            estimated_candidates,
            input_domains: domains.clone(),
            predicates: Vec::new(),
        });
        register_pattern(pattern, &mut domains)?;
    }
    Ok(PlannedBranch {
        anchors,
        filters: Vec::new(),
        patterns: planned,
        domains,
        scope,
    })
}

fn access_path(view: &ReadView, pattern: &PositPattern, available: &HashSet<&str>) -> PlanAccess {
    if pattern.effect.is_some() {
        return PlanAccess::EffectiveSlice;
    }
    let mut roles = Vec::new();
    let mut bindings = Vec::new();
    let mut add = |name: &String| {
        if available.contains(name.as_str()) {
            bindings.push(name.clone());
        }
    };
    if pattern.cutoff.is_none()
        && let Some(name) = &pattern.posit
    {
        add(name);
    }
    if let Some(name) = &pattern.appearances.binding {
        add(name);
    }
    for member in &pattern.appearances.members {
        match &member.role {
            RoleSlot::Named(name) => roles.push(name.clone()),
            RoleSlot::Variable(name) => add(name),
            RoleSlot::Wildcard => {}
        }
        match &member.thing {
            ThingSlot::Variable(name) | ThingSlot::LegacyVariable(name) => add(name),
            ThingSlot::OneOf(names) => {
                for name in names {
                    add(name);
                }
            }
            ThingSlot::Identity(_) | ThingSlot::Wildcard => {}
        }
    }
    roles.sort();
    roles.dedup();
    bindings.sort();
    bindings.dedup();
    if roles.is_empty()
        && bindings.is_empty()
        && indexed_literal(view, pattern).is_none()
        && !pattern
            .appearances
            .members
            .iter()
            .any(|m| matches!(m.thing, ThingSlot::Identity(_)))
    {
        PlanAccess::FullScan
    } else {
        PlanAccess::Bitmap { roles, bindings }
    }
}

fn schema(domains: &HashMap<String, Domain>) -> Vec<PlanBinding> {
    let mut bindings: Vec<_> = domains
        .iter()
        .map(|(name, domain)| PlanBinding {
            name: name.clone(),
            domain: (*domain).into(),
        })
        .collect();
    bindings.sort_by(|a, b| a.name.cmp(&b.name));
    bindings
}

fn value_slot(slot: &ValueSlot) -> String {
    match slot {
        ValueSlot::Wildcard => "*".into(),
        ValueSlot::Variable(name) | ValueSlot::LegacyVariable(name) => format!("?{name}"),
        ValueSlot::Literal(value) => value.token().to_string(),
    }
}

fn time_slot(slot: &TimeSlot) -> String {
    match slot {
        TimeSlot::Wildcard => "*".into(),
        TimeSlot::Variable(name) | TimeSlot::LegacyVariable(name) => format!("?{name}"),
        TimeSlot::Literal(time) => time.to_string(),
    }
}

fn appearances(pattern: &AppearanceSetPattern) -> String {
    let prefix = pattern
        .binding
        .as_ref()
        .map(|name| format!("?{name} = "))
        .unwrap_or_default();
    if pattern.any {
        return format!("{prefix}*");
    }
    let mut members: Vec<_> = pattern
        .members
        .iter()
        .map(|member| {
            let thing = match &member.thing {
                ThingSlot::Wildcard => "*".into(),
                ThingSlot::Identity(id) => id.to_string(),
                ThingSlot::Variable(name) | ThingSlot::LegacyVariable(name) => format!("?{name}"),
                ThingSlot::OneOf(names) => names
                    .iter()
                    .map(|name| format!("?{name}"))
                    .collect::<Vec<_>>()
                    .join("|"),
            };
            let role = match &member.role {
                RoleSlot::Wildcard => "*".into(),
                RoleSlot::Named(name) => format!("`{name}`"),
                RoleSlot::Variable(name) => format!("?{name}"),
            };
            format!("({thing}, {role})")
        })
        .collect();
    if pattern.open {
        members.push("...".into());
    }
    format!("{prefix}{{{}}}", members.join(", "))
}

fn pattern_text(pattern: &PositPattern) -> String {
    let prefix = pattern
        .posit
        .as_ref()
        .map(|name| format!("?{name} = "))
        .unwrap_or_default();
    format!(
        "{prefix}[{}, {}, {}]",
        appearances(&pattern.appearances),
        value_slot(&pattern.value),
        time_slot(&pattern.time)
    )
}

impl PlannedPattern {
    fn describe(&self) -> PlanNode {
        let p = &self.pattern;
        let mut output_domains = self.input_domains.clone();
        register_pattern(p, &mut output_domains)
            .expect("pattern domains validated during planning");
        let bindings = schema(&output_domains);
        let mut dependencies: Vec<_> = p.dependencies().into_iter().map(str::to_string).collect();
        dependencies.sort();
        if let Some(effect) = &p.effect {
            return PlanNode {
                operation: PlanOperator::ResolveEffect {
                    assertion_cutoff: (&effect.assertion_cutoff).into(),
                    appearance_cutoff: (&effect.appearance_cutoff).into(),
                    estimated_assertion_candidates: self.estimated_candidates,
                    dependencies,
                },
                bindings: Vec::new(),
                inputs: Vec::new(),
            }
            .wrap(PlanOperator::MatchEffectiveTarget {
                source_pattern: self.source_pattern,
                target: pattern_text(p),
                via: effect.via.as_deref().map(pattern_text),
            })
            .with_bindings(bindings);
        }
        let mut node = PlanNode {
            operation: PlanOperator::StructuralScan {
                source_pattern: self.source_pattern,
                access: self.access.clone(),
                estimated_candidates: self.estimated_candidates,
                exact: !p.appearances.any && !p.appearances.open,
                appearances: appearances(&p.appearances),
                dependencies,
            },
            bindings: schema(&self.input_domains),
            inputs: Vec::new(),
        };
        // Structural bindings exist before temporal reduction. Field bindings
        // are added by MatchFields, on the correct side of that reduction.
        let mut structural = p.clone();
        structural.posit = None;
        structural.value = ValueSlot::Wildcard;
        structural.time = TimeSlot::Wildcard;
        let mut structural_domains = self.input_domains.clone();
        let deferred = deferred_temporal_fields(p);
        structural_domains.retain(|name, _| !deferred.contains(name.as_str()));
        register_pattern(&structural, &mut structural_domains)
            .expect("structural domains validated");
        node.bindings = schema(&structural_domains);
        if let Some(cutoff) = &p.cutoff
            && !p.latest_matching
        {
            node = node.wrap(PlanOperator::MaximalByAppearanceSet {
                cutoff: cutoff.into(),
            });
        }
        node = node
            .wrap(PlanOperator::MatchFields {
                posit: p.posit.clone(),
                value: value_slot(&p.value),
                time: time_slot(&p.time),
            })
            .with_bindings(bindings);
        if let Some(cutoff) = &p.cutoff
            && p.latest_matching
        {
            node = node.wrap(PlanOperator::MaximalByAppearanceSet {
                cutoff: cutoff.into(),
            });
        }
        if p.cutoff.is_some() {
            let mut variables: Vec<_> = deferred
                .into_iter()
                .filter(|name| self.input_domains.contains_key(*name))
                .map(str::to_string)
                .collect();
            variables.sort();
            variables.dedup();
            if !variables.is_empty() {
                node = node.wrap(PlanOperator::JoinExternalFields { variables });
            }
        }
        if !self.predicates.is_empty() {
            node = node.wrap(filter_operator(&self.predicates));
        }
        node
    }
}

impl PlanNode {
    fn with_bindings(mut self, bindings: Vec<PlanBinding>) -> Self {
        self.bindings = bindings;
        self
    }
}

impl PlannedBranch {
    fn describe(&self) -> PlanNode {
        let join = PlanNode {
            operation: PlanOperator::NestedLoopJoin {
                scope: self.scope.clone(),
                pattern_order: self.patterns.iter().map(|p| p.source_pattern).collect(),
            },
            bindings: schema(&self.domains),
            inputs: self.patterns.iter().map(PlannedPattern::describe).collect(),
        };
        if self.filters.is_empty() {
            join
        } else {
            join.wrap(filter_operator(&self.filters))
        }
    }
}

impl PhysicalQuery {
    pub(super) fn describe(&self, execution: &ExecutionContext) -> QueryPlan {
        let mut inputs: Vec<_> = self.branches.iter().map(PlannedBranch::describe).collect();
        let mut root = if inputs.len() == 1 {
            inputs.remove(0)
        } else {
            PlanNode {
                operation: PlanOperator::UnionAll,
                bindings: schema(&self.domains),
                inputs,
            }
        };
        for block in &self.not_exists {
            let mut correlated: Vec<_> = block
                .patterns
                .iter()
                .flat_map(|p| {
                    p.pattern
                        .introduced()
                        .into_iter()
                        .chain(p.pattern.dependencies())
                })
                .filter(|name| self.domains.contains_key(*name))
                .map(str::to_string)
                .collect();
            for predicate in &block.filters {
                for name in predicate.variables() {
                    if self.domains.contains_key(name) {
                        correlated.push(name.to_owned());
                    }
                }
            }
            correlated.sort();
            correlated.dedup();
            root = PlanNode {
                operation: PlanOperator::AntiJoin { correlated },
                bindings: root.bindings.clone(),
                inputs: vec![root, block.describe()],
            };
        }
        if self.materialization_reason.is_some() && !self.predicates.is_empty() {
            root = root.wrap(filter_operator(&self.predicates));
        }
        if self.returns.is_empty() {
            root = root.wrap(if self.materialization_reason.is_none() {
                PlanOperator::Consume
            } else {
                PlanOperator::IdentityBindings
            });
        } else {
            root = root.wrap(PlanOperator::Project {
                columns: self.returns.clone(),
            });
            root.bindings = self
                .returns
                .iter()
                .map(|name| PlanBinding {
                    name: name.clone(),
                    domain: self.domains[name].into(),
                })
                .collect();
            if self.distinct {
                root = root.wrap(PlanOperator::Distinct);
            }
            if !self.order.is_empty() {
                let keys = self
                    .order
                    .iter()
                    .map(|key| PlanOrder {
                        variable: key.variable.clone(),
                        descending: key.descending,
                    })
                    .collect();
                root = root.wrap(match self.limit.and_then(|limit| limit.checked_add(1)) {
                    Some(rows) if self.materialization_reason.is_none() => {
                        PlanOperator::TopK { keys, rows }
                    }
                    _ => PlanOperator::Sort { keys },
                });
            }
            if let Some(rows) = self.limit {
                root = root.wrap(PlanOperator::Limit {
                    rows,
                    presentation_only: self.materialization_reason.is_some()
                        || !self.order.is_empty(),
                });
            }
        }
        QueryPlan {
            version: QUERY_PLAN_VERSION,
            resolved_now: execution.metadata.resolved_now.to_string(),
            incremental: self.materialization_reason.is_none(),
            materialization_reason: self.materialization_reason.clone(),
            root,
        }
    }
}

fn filter_operator(predicates: &[Predicate]) -> PlanOperator {
    PlanOperator::Filter {
        predicates: predicates
            .iter()
            .map(|p| {
                let (left, operator, right) = match p {
                    Predicate::Calendar(test) => return test.source.clone(),
                    Predicate::Comparison {
                        left,
                        operator,
                        right,
                    } => (left, operator, right),
                };
                let right = match right {
                    Operand::Variable(name) => format!("?{name}"),
                    Operand::Parameter(name) => name.clone(),
                    Operand::Identity(value) => value.result_cell().text,
                    Operand::Literal(value) => value.token().to_string(),
                    Operand::Time(time) => format!("'{time}'"),
                };
                format!("?{left} {operator} {right}")
            })
            .collect(),
    }
}
