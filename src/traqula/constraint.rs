//! Saved constraints use ordinary classified Things and unary source posits.
use super::*;
use crate::construct::{Appearance, AppearanceSet, LiteralPositSpec, Posit, Role};
use crate::datatype::temporal::retain_maximal;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::hash::Hasher;

mod definition;
use definition::{Definition, FindingKind};

type Record = Arc<Posit<LiteralValue>>;
type Result<T> = std::result::Result<T, DatabaseError>;
/// Version of the saved definition contract.
pub const CONSTRAINT_VERSION: u16 = 1;
/// Version of the finding report returned by a check.
pub const CONSTRAINT_REPORT_VERSION: u16 = 2;

pub(super) struct ReadTrace {
    ids: BTreeSet<Thing>,
    units: u64,
    memory: budget::Reservation,
}

impl ReadTrace {
    pub(super) fn work(&mut self, units: u64) {
        self.units = self.units.saturating_add(units);
    }
}

impl ExecutionContext {
    pub(super) fn trace_read(&self, id: Thing) -> Result<()> {
        if let Some(trace) = &self.trace {
            let mut trace = trace.borrow_mut();
            if trace.ids.insert(id) {
                trace.memory.grow(48)?;
            }
        }
        Ok(())
    }
}

fn invalid(message: impl Into<String>) -> DatabaseError {
    DatabaseError::Execution(message.into())
}

fn string_value(value: &str) -> Result<LiteralValue> {
    let token = serde_json::to_string(value).map_err(|e| invalid(e.to_string()))?;
    LiteralValue::new(token, LiteralFamily::String).map_err(invalid)
}

fn decoded(token: &str) -> Result<String> {
    serde_json::from_str(token).map_err(|_| invalid("expected a Traqula string"))
}

fn has_string(record: &Record, value: &str) -> bool {
    record.value().family() == LiteralFamily::String
        && serde_json::from_str::<String>(record.value().token()).is_ok_and(|s| s == value)
}

fn member(record: &Record, role: &str) -> Option<Thing> {
    record
        .appearance_set()
        .appearances()
        .iter()
        .find(|a| a.role_name() == role)
        .map(|a| a.thing())
}

fn unary(record: &Record, role: &str) -> Option<Thing> {
    (record.appearance_set().appearances().len() == 1)
        .then(|| member(record, role))
        .flatten()
}

fn slot<const N: usize>(
    view: &ReadView,
    members: [(&str, Thing); N],
    execution: &ExecutionContext,
) -> Result<Vec<Record>> {
    let mut appearances = Vec::with_capacity(N);
    for (name, id) in members {
        let Some(role) = view.normalized_role(name) else {
            return Ok(Vec::new());
        };
        appearances.push(Arc::new(Appearance::new(id, role)));
    }
    let set = AppearanceSet::checked(appearances)?;
    let mut records = Vec::new();
    if let Some(posting) = view.set_posting(&set) {
        for id in posting.iter() {
            execution.work(1)?;
            execution.trace_read(id)?;
            records.push(
                view.posit(id)
                    .ok_or_else(|| DatabaseError::Invariant("missing indexed posit".into()))?,
            );
        }
    }
    Ok(records)
}

fn named(view: &ReadView, name: &str, execution: &ExecutionContext) -> Result<BTreeSet<Thing>> {
    let mut ids = BTreeSet::new();
    let Some(names) = view
        .normalized_role("name")
        .and_then(|r| view.role_posting(r.role()))
    else {
        return Ok(ids);
    };
    let string = string_value(name)?;
    let Some(values) = view.value_posting(string.nominal_fingerprint()) else {
        return Ok(ids);
    };
    let candidates: Box<dyn Iterator<Item = Thing> + '_> = if names.len() < values.len() {
        Box::new(names.iter())
    } else {
        Box::new(values.iter())
    };
    for id in candidates {
        execution.work(1)?;
        if !names.contains(id) || !values.contains(id) {
            continue;
        }
        execution.trace_read(id)?;
        let record = view
            .posit(id)
            .ok_or_else(|| DatabaseError::Invariant("missing named posit".into()))?;
        if let Some(subject) = unary(&record, "name")
            && has_string(&record, name)
        {
            ids.insert(subject);
        }
    }
    Ok(ids)
}

fn classes(view: &ReadView, execution: &ExecutionContext) -> Result<Vec<Thing>> {
    let mut classes = Vec::new();
    for id in named(view, "Constraint", execution)? {
        if slot(view, [("class", id)], execution)?
            .iter()
            .any(|p| has_string(p, "declared"))
        {
            classes.push(id);
        }
    }
    Ok(classes)
}

fn is_member(
    view: &ReadView,
    policy: Thing,
    class: Thing,
    execution: &ExecutionContext,
) -> Result<bool> {
    Ok(
        slot(view, [("thing", policy), ("class", class)], execution)?
            .iter()
            .any(|p| has_string(p, "included")),
    )
}

fn one(ids: &[Thing], label: &str) -> Result<Option<Thing>> {
    match ids {
        [] => Ok(None),
        [id] => Ok(Some(*id)),
        _ => Err(invalid(format!("ambiguous {label}: candidates {ids:?}"))),
    }
}

fn named_policy(
    view: &ReadView,
    name: &str,
    class: Thing,
    execution: &ExecutionContext,
) -> Result<Option<Thing>> {
    let mut policies = Vec::new();
    for id in named(view, name, execution)? {
        if is_member(view, id, class, execution)? {
            policies.push(id);
        }
    }
    one(&policies, &format!("constraint name {name:?}"))
}

fn verify_policy(view: &ReadView, policy: Thing, execution: &ExecutionContext) -> Result<()> {
    for class in classes(view, execution)? {
        if is_member(view, policy, class, execution)? {
            return Ok(());
        }
    }
    Err(invalid(format!(
        "Thing {policy} is not a member of a declared Constraint class"
    )))
}

fn resolve_id(
    pair: Pair<'_, Rule>,
    expected: ResultCellKind,
    variables: &Variables,
    store: &str,
    execution: &ExecutionContext,
) -> Result<Thing> {
    if pair.as_rule() == Rule::parameter {
        return match execution.parameter(pair.as_str())? {
            ResolvedParameter::Identity {
                kind,
                store: origin,
                id,
            } if *kind == expected && origin == store => Ok(*id),
            _ => Err(DatabaseError::Parameter(format!(
                "{} requires a {expected:?} reference into store {store}",
                pair.as_str()
            ))),
        };
    }
    let name = nested_mutation_variable(pair)?;
    let bound = variables
        .values
        .get(&name)
        .ok_or_else(|| DatabaseError::UnknownVariable(name.clone()))?;
    one(&result_set_identities(bound)?, &format!("binding {name}"))?
        .ok_or_else(|| invalid(format!("empty binding {name}")))
}

fn selected_class(
    view: &ReadView,
    clause: Option<Pair<'_, Rule>>,
    variables: &Variables,
    store: &str,
    execution: &ExecutionContext,
) -> Result<Option<Thing>> {
    let candidates = classes(view, execution)?;
    if let Some(clause) = clause {
        let class = resolve_id(
            clause.into_inner().next().unwrap(),
            ResultCellKind::Thing,
            variables,
            store,
            execution,
        )?;
        if !candidates.contains(&class) {
            return Err(invalid(format!(
                "Thing {class} is not a declared class named Constraint"
            )));
        }
        Ok(Some(class))
    } else {
        one(&candidates, "Constraint class")
    }
}

fn command_time(clause: Pair<'_, Rule>, execution: &ExecutionContext) -> Result<Time> {
    let value = clause
        .into_inner()
        .next()
        .unwrap()
        .into_inner()
        .next()
        .unwrap();
    if value.as_rule() == Rule::parameter {
        return execution.parameter(value.as_str())?.time().ok_or_else(|| {
            DatabaseError::Parameter(format!("{} requires a time", value.as_str()))
        });
    }
    parse_time_with_now(value.as_str(), &execution.metadata.resolved_now)
        .ok_or_else(|| invalid(format!("invalid constraint time {}", value.as_str())))
}

fn verify_metadata(
    view: &ReadView,
    revision: Thing,
    expected: &Value,
    execution: &ExecutionContext,
) -> Result<Vec<Thing>> {
    let mut ids = Vec::new();
    for record in slot(view, [("language", revision)], execution)? {
        if record.value().family() != LiteralFamily::Json
            || serde_json::from_str::<Value>(record.value().token())
                .ok()
                .as_ref()
                != Some(expected)
        {
            return Err(invalid(format!(
                "conflicting constraint metadata at Posit {} for revision {revision}",
                record.posit()
            )));
        }
        ids.push(record.posit());
    }
    Ok(ids)
}

struct Catalog<'a> {
    db: &'a Database,
    pending: Vec<Role>,
}

impl Catalog<'_> {
    fn role(&mut self, view: &ReadView, name: &str) -> Result<Arc<Role>> {
        if let Some(role) = view.normalized_role(name) {
            return Ok(role);
        }
        if let Some(role) = self.pending.iter().find(|role| role.name() == name) {
            return Ok(Arc::new(role.clone()));
        }
        let role = Role::new(self.db.reserve_identity()?, name.to_owned(), false);
        self.pending.push(role.clone());
        Ok(Arc::new(role))
    }

    fn record<const N: usize>(
        &mut self,
        view: &ReadView,
        members: [(&str, Thing); N],
        value: LiteralValue,
        time: &Time,
    ) -> Result<LiteralPositSpec> {
        let mut appearances = Vec::with_capacity(N);
        for (name, id) in members {
            appearances.push(Arc::new(Appearance::new(id, self.role(view, name)?)));
        }
        Ok((AppearanceSet::checked(appearances)?, value, time.clone()))
    }
}

impl Engine<'_> {
    pub(super) fn constraint_command(
        &self,
        view: &mut ScriptView<'_>,
        command: Pair<'_, Rule>,
        variables: &mut Variables,
        sink: &mut dyn RowSink,
        columns: &mut Option<Vec<String>>,
        execution: &ExecutionContext,
    ) -> Result<query::SearchOutcome> {
        let store = self.database.store_id();
        let (policy, revision, report, audit) = match command.as_rule() {
            Rule::add_constraint => {
                let (policy, revision) =
                    self.save_constraint(view.write()?, command, variables, execution)?;
                (policy, revision, None, None)
            }
            _ => {
                if execution.max_rows_per_search == Some(0) {
                    return Err(DatabaseError::ResourceLimit {
                        resource: "constraint report rows",
                        limit: 0,
                    });
                }
                let evaluator = command
                    .clone()
                    .into_inner()
                    .find(|p| p.as_rule() == Rule::audit_clause)
                    .map(|clause| {
                        resolve_id(
                            clause.into_inner().next().unwrap(),
                            ResultCellKind::Thing,
                            variables,
                            store,
                            execution,
                        )
                    })
                    .transpose()?;
                if let Some(id) = evaluator
                    && view.read().appearances(id).is_none()
                    && view.read().posit(id).is_none()
                {
                    return Err(invalid(format!("unknown evaluator Thing {id}")));
                }
                let (policy, revision, report) =
                    self.check_constraint(view.read(), command, variables, execution)?;
                let audit = match evaluator {
                    Some(id) => {
                        Some(self.record_check(view.write()?, revision, id, &report, execution)?)
                    }
                    None => None,
                };
                (policy, revision, Some(report), audit)
            }
        };
        let mut names = vec!["policy".into(), "revision".into(), "store".into()];
        let mut row = vec![
            ResultCell::new(ResultCellKind::Thing, policy.to_string()),
            ResultCell::new(ResultCellKind::Posit, revision.to_string()),
            ResultCell::new(ResultCellKind::Literal, string_value(store)?.token()),
        ];
        if let Some(report) = report {
            names.push("report".into());
            row.push(ResultCell::new(ResultCellKind::Literal, report));
        }
        if let Some(id) = audit {
            names.push("audit".into());
            row.push(ResultCell::new(ResultCellKind::Posit, id.to_string()));
        }
        let stopped = matches!(sink.on_meta(&names), SinkFlow::Stop)
            || matches!(sink.push(row), SinkFlow::Stop);
        *columns = Some(names);
        Ok(query::SearchOutcome {
            matched_rows: 1,
            identity_bindings: HashMap::default(),
            has_return: true,
            sink_stopped: stopped,
            _identity_memory: Default::default(),
        })
    }

    fn record_check(
        &self,
        view: &mut ReadView,
        revision: Thing,
        evaluator: Thing,
        report: &str,
        execution: &ExecutionContext,
    ) -> Result<Thing> {
        let now = &execution.metadata.resolved_now;
        let mut catalog = Catalog {
            db: self.database,
            pending: Vec::new(),
        };
        let report = LiteralValue::new(report, LiteralFamily::Json).map_err(invalid)?;
        let record = catalog.record(view, [("policy", revision)], report, now)?;
        let posit = catalog.role(view, "posit")?;
        let ascertains = catalog.role(view, "ascertains")?;
        execution.check()?;
        let created =
            self.database
                .create_literal_batch(view, catalog.pending, vec![record], |records| {
                    let set = AppearanceSet::checked(vec![
                        Arc::new(Appearance::new(records[0].posit(), posit)),
                        Arc::new(Appearance::new(evaluator, ascertains)),
                    ])?;
                    Ok(vec![(
                        set,
                        LiteralValue::new("100%", LiteralFamily::Certainty).map_err(invalid)?,
                        now.clone(),
                    )])
                })?;
        Ok(created.targets[0].posit())
    }

    fn save_constraint(
        &self,
        view: &mut ReadView,
        command: Pair<'_, Rule>,
        variables: &mut Variables,
        execution: &ExecutionContext,
    ) -> Result<(Thing, Thing)> {
        let mut parts = command.into_inner().peekable();
        let binding = parts
            .next_if(|p| p.as_rule() == Rule::insert)
            .map(nested_mutation_variable)
            .transpose()?;
        let target = parts.next().unwrap();
        let class_clause = parts.next_if(|p| p.as_rule() == Rule::constraint_class);
        let now = &execution.metadata.resolved_now;
        let starts = parts
            .next_if(|p| p.as_rule() == Rule::constraint_start)
            .map(|p| command_time(p, execution))
            .transpose()?
            .unwrap_or_else(|| now.clone());
        let braced = parts.next().unwrap().as_str();
        let source = &braced[1..braced.len() - 1];
        let metadata = Definition::parse(source)?.validate(execution)?;
        let mut catalog = Catalog {
            db: self.database,
            pending: Vec::new(),
        };
        let mut targets = Vec::new();
        let policy = if target.as_rule() == Rule::constraint_policy {
            let id = resolve_id(
                target.into_inner().next().unwrap(),
                ResultCellKind::Thing,
                variables,
                self.database.store_id(),
                execution,
            )?;
            verify_policy(view, id, execution)?;
            id
        } else {
            let name = decoded(target.as_str())?;
            let class = match selected_class(
                view,
                class_clause,
                variables,
                self.database.store_id(),
                execution,
            )? {
                Some(class) => class,
                None => {
                    let class = self.database.reserve_identity()?;
                    targets.push(catalog.record(
                        view,
                        [("class", class)],
                        string_value("declared")?,
                        now,
                    )?);
                    targets.push(catalog.record(
                        view,
                        [("name", class)],
                        string_value("Constraint")?,
                        now,
                    )?);
                    class
                }
            };
            match named_policy(view, &name, class, execution)? {
                Some(policy) => policy,
                None => {
                    let policy = self.database.reserve_identity()?;
                    targets.push(catalog.record(
                        view,
                        [("thing", policy), ("class", class)],
                        string_value("included")?,
                        now,
                    )?);
                    targets.push(catalog.record(
                        view,
                        [("name", policy)],
                        string_value(&name)?,
                        now,
                    )?);
                    policy
                }
            }
        };
        let value = string_value(source)?;
        if let Some(old) = slot(view, [("source", policy)], execution)?
            .iter()
            .find(|p| p.time() == &starts && p.value() == &value)
        {
            verify_metadata(view, old.posit(), &metadata, execution)?;
        }
        targets.push(catalog.record(view, [("source", policy)], value, &starts)?);
        let metadata =
            LiteralValue::new(metadata.to_string(), LiteralFamily::Json).map_err(invalid)?;
        let language = catalog.role(view, "language")?;
        execution.check()?;
        let created =
            self.database
                .create_literal_batch(view, catalog.pending, targets, |targets| {
                    let revision = targets.last().unwrap().posit();
                    let set = AppearanceSet::checked(vec![Arc::new(Appearance::new(
                        revision, language,
                    ))])?;
                    Ok(vec![(set, metadata, starts)])
                })?;
        let revision = created.targets.last().unwrap().posit();
        if let Some(name) = binding {
            bind_identity(variables, name, revision);
        }
        Ok((policy, revision))
    }

    fn check_constraint(
        &self,
        view: &ReadView,
        command: Pair<'_, Rule>,
        variables: &Variables,
        execution: &ExecutionContext,
    ) -> Result<(Thing, Thing, String)> {
        let mut checked = execution.clone();
        let trace = std::rc::Rc::new(std::cell::RefCell::new(ReadTrace {
            ids: BTreeSet::new(),
            units: 0,
            memory: execution.workspace(|| 0)?,
        }));
        checked.trace = Some(trace.clone());
        let execution = &checked;
        let store = self.database.store_id();
        let mut parts = command.into_inner().peekable();
        let (policy, revision, cutoff) =
            resolve_revision(view, &mut parts, variables, store, execution)?;
        let revision_id = revision.posit();
        let source = decoded(revision.value().token())?;
        let definition = Definition::parse(&source)?;
        let mut exhaustive = execution.clone();
        exhaustive.max_rows_per_search = None;
        for (name, id, kind) in [
            ("constraint_revision", revision_id, ResultCellKind::Posit),
            ("constraint_policy", policy, ResultCellKind::Thing),
        ] {
            if definition.parameters.contains(name) {
                exhaustive.parameters.insert(
                    name.into(),
                    ResolvedParameter::Identity {
                        kind,
                        id,
                        store: store.into(),
                    },
                );
            }
        }
        let prepared = definition.prepare(&exhaustive)?;
        let metadata = definition.metadata(&prepared)?;
        let declarations = verify_metadata(view, revision_id, &metadata, execution)?;
        let readiness = parts
            .next_if(|p| p.as_rule() == Rule::readiness)
            .map(|p| {
                resolve_id(
                    p.into_inner().next().unwrap(),
                    ResultCellKind::Posit,
                    variables,
                    store,
                    execution,
                )
            })
            .transpose()?;
        let mut parameters = BTreeMap::new();
        let full_manifest = parts
            .next_if(|p| p.as_rule() == Rule::full_manifest)
            .is_some();
        for name in &definition.parameters {
            if matches!(name.as_str(), "constraint_revision" | "constraint_policy") {
                continue;
            }
            let parameter = match exhaustive.parameter(name)? {
                ResolvedParameter::Time { literal, .. } => {
                    json!({"kind":"time","text":literal.token()})
                }
                ResolvedParameter::Literal(value) => json!({"kind":"literal","text":value.token()}),
                ResolvedParameter::Identity { kind, id, store } => {
                    json!({"kind":kind, "text":format!("{store}/{id}")})
                }
            };
            parameters.insert(name, parameter);
        }
        let scope = json!({"parameters":parameters, "clock": definition.uses_now.then(|| execution.metadata.resolved_now.to_string())});
        let pending = readiness
            .map(|id| pending_deliveries(view, id, revision_id, &scope, execution))
            .transpose()?
            .unwrap_or_default();
        let complete = readiness.is_some() && pending.is_empty();
        let mut findings = Vec::new();
        let mut evidence = Vec::new();
        let mut alternative_subjects = BTreeSet::new();
        let mut tentative = Vec::new();
        let mut report_memory = execution.workspace(|| 0)?;
        let policy_names = slot(view, [("name", policy)], execution)?
            .into_iter()
            .filter(|p| p.value().family() == LiteralFamily::String)
            .map(|p| decoded(p.value().token()))
            .collect::<Result<BTreeSet<_>>>()?;
        for (index, (spec, query)) in definition.queries.iter().zip(prepared).enumerate() {
            let mut rows = Findings::new(execution);
            query::execute_prepared(view, query, &mut rows, &mut None, &exhaustive)?;
            if let Some(error) = rows.error {
                return Err(error);
            }
            let expected = spec
                .expected
                .as_ref()
                .or_else(|| policy_names.first())
                .map(String::as_str);
            for row in rows.rows {
                execution.work(1)?;
                use budget::Workspace;
                report_memory.grow(row.heap_bytes().saturating_mul(3).saturating_add(512))?;
                let subject = row[0].clone();
                let witnesses = row
                    .iter()
                    .filter(|cell| cell.kind == ResultCellKind::Posit)
                    .cloned()
                    .collect();
                let mut finding = Finding {
                    query: index,
                    subject,
                    status: "evidence",
                    expected,
                    observed: row,
                    witnesses,
                };
                match spec.kind {
                    FindingKind::Permits => {
                        alternative_subjects.insert(finding.subject.text.clone());
                        evidence.push(finding);
                    }
                    FindingKind::Evidence => evidence.push(finding),
                    FindingKind::Forbids => tentative.push(finding),
                    FindingKind::Unresolved => {
                        alternative_subjects.insert(finding.subject.text.clone());
                        finding.status = "indeterminate";
                        findings.push(finding);
                    }
                    FindingKind::Violation => {
                        finding.status = if complete || !spec.needs_readiness {
                            "violated"
                        } else {
                            "indeterminate"
                        };
                        findings.push(finding);
                    }
                }
            }
        }
        // Opposing interpretations of one action can change its verdict.
        // A permitted interpretation of one subject cannot excuse another.
        for mut finding in tentative {
            execution.work(1)?;
            finding.status = if complete && !alternative_subjects.contains(&finding.subject.text) {
                "violated"
            } else {
                "indeterminate"
            };
            findings.push(finding);
        }
        let verdict = if findings.iter().any(|f| f.status == "violated") {
            "violated"
        } else if !complete || !findings.is_empty() {
            "indeterminate"
        } else {
            "satisfied"
        };
        let ids = |ids: &[Thing]| ids.iter().map(Thing::to_string).collect::<Vec<_>>();
        let count = view.posit_count();
        let cached_digest = view.manifest_digest();
        let mut snapshot = json!({"count": count});
        let digest = if let Some(digest) = cached_digest.filter(|_| !full_manifest) {
            digest
        } else {
            report_memory.grow(count as u64 * if full_manifest { 64 } else { 8 })?;
            execution.work(budget::sort_work(count, 1))?;
            let mut manifest: Vec<_> = view.posit_ids().collect();
            manifest.sort_unstable();
            let digest = match cached_digest {
                Some(digest) => digest,
                None => {
                    let mut hash = seahash::SeaHasher::new();
                    for id in &manifest {
                        execution.work(1)?;
                        hash.write(&id.to_le_bytes());
                    }
                    view.cache_manifest_digest(hash)
                }
            };
            if full_manifest {
                execution.work(count as u64)?;
                snapshot["posits"] = json!(ids(&manifest));
            }
            digest
        };
        snapshot["digest"] = json!(format!("{digest:016x}"));
        let trace = trace.borrow();
        let report = json!({
            "version": CONSTRAINT_REPORT_VERSION,
            "verdict": verdict,
            "store": store,
            "policy": policy.to_string(),
            "revision": revision_id.to_string(),
            "source_digest": format!("{:016x}", seahash::hash(source.as_bytes())),
            "policy_at": cutoff.map(|t| t.to_string()),
            "revision_at": revision.time().to_string(),
            "now": execution.metadata.resolved_now.to_string(),
            "scope": scope,
            "definition": metadata,
            "declarations": ids(&declarations),
            "read_posits": trace.ids.iter().map(Thing::to_string).collect::<Vec<_>>(),
            "work_units": trace.units,
            "snapshot": snapshot,
            "readiness": readiness.map(|id|id.to_string()),
            "complete": complete,
            "missing_inputs": ids(&pending),
            "findings": findings,
            "evidence": evidence
        })
        .to_string();
        execution.check()?;
        Ok((policy, revision_id, report))
    }
}

#[derive(serde::Serialize)]
struct Finding<'a> {
    query: usize,
    subject: ResultCell,
    status: &'static str,
    expected: Option<&'a str>,
    observed: Vec<ResultCell>,
    witnesses: Vec<ResultCell>,
}

fn resolve_revision(
    view: &ReadView,
    parts: &mut std::iter::Peekable<Pairs<'_, Rule>>,
    variables: &Variables,
    store: &str,
    execution: &ExecutionContext,
) -> Result<(Thing, Record, Option<Time>)> {
    let target = parts.next().unwrap();
    let selection = if target.as_rule() == Rule::string {
        let name = decoded(target.as_str())?;
        let unknown = || invalid(format!("unknown constraint {name:?}"));
        let class = selected_class(
            view,
            parts.next_if(|p| p.as_rule() == Rule::constraint_class),
            variables,
            store,
            execution,
        )?
        .ok_or_else(unknown)?;
        let policy = named_policy(view, &name, class, execution)?.ok_or_else(unknown)?;
        let cut = parts
            .next_if(|p| p.as_rule() == Rule::constraint_cut)
            .map(|p| command_time(p, execution))
            .transpose()?
            .unwrap_or_else(|| execution.metadata.resolved_now.clone());
        let mut sources = slot(view, [("source", policy)], execution)?;
        if sources.is_empty() {
            return Err(invalid(format!(
                "incomplete constraint {name:?}: no source"
            )));
        }
        sources.retain(|p| p.time().definitely_at_or_before(&cut));
        let maxima = retain_maximal(sources, |p| p.time(), |_| execution.work(1))?;
        match maxima.as_slice() {
            [] => {
                return Err(invalid(format!(
                    "no applicable revision of {name:?} as of {cut}"
                )));
            }
            [selected] => (policy, selected.clone(), Some(cut)),
            _ => {
                return Err(invalid(format!(
                    "ambiguous constraint revision for {name:?} as of {cut}: candidates {:?}",
                    maxima.iter().map(|p| p.posit()).collect::<Vec<_>>()
                )));
            }
        }
    } else {
        let id = resolve_id(target, ResultCellKind::Posit, variables, store, execution)?;
        execution.trace_read(id)?;
        let p = view
            .posit(id)
            .ok_or_else(|| invalid(format!("unknown revision Posit {id}")))?;
        let policy = unary(&p, "source")
            .ok_or_else(|| invalid(format!("Posit {id} is not a unary source revision")))?;
        verify_policy(view, policy, execution)?;
        (policy, p, None)
    };
    Ok(selection)
}

struct Findings<'a> {
    execution: &'a ExecutionContext,
    rows: budget::BudgetVec<Vec<ResultCell>>,
    error: Option<DatabaseError>,
}

impl<'a> Findings<'a> {
    fn new(execution: &'a ExecutionContext) -> Self {
        Self {
            execution,
            rows: budget::BudgetVec::new(&execution.budget),
            error: None,
        }
    }
}

impl RowSink for Findings<'_> {
    fn push(&mut self, row: Vec<ResultCell>) -> SinkFlow {
        let result = if self
            .execution
            .max_rows_per_search
            .is_some_and(|limit| self.rows.len() >= limit)
        {
            Err(DatabaseError::ResourceLimit {
                resource: "constraint findings",
                limit: self.rows.len() as u64,
            })
        } else {
            self.rows.push(row)
        };
        match result {
            Ok(()) => SinkFlow::Continue,
            Err(e) => {
                self.error = Some(e);
                SinkFlow::Stop
            }
        }
    }
}

// Each immutable manifest link names a delivered Posit and optionally a predecessor.
fn pending_deliveries(
    view: &ReadView,
    id: Thing,
    revision: Thing,
    scope: &Value,
    execution: &ExecutionContext,
) -> Result<Vec<Thing>> {
    execution.trace_read(id)?;
    let declaration = view
        .posit(id)
        .ok_or_else(|| invalid(format!("unknown readiness declaration {id}")))?;
    if member(&declaration, "policy") != Some(revision)
        || declaration.appearance_set().appearances().len() != 2
    {
        return Err(invalid(
            "readiness declaration does not cover this revision",
        ));
    }
    let unparameterized = scope["parameters"] == json!({}) && scope["clock"].is_null();
    let covered = (unparameterized && has_string(&declaration, "ready"))
        || serde_json::from_str::<Value>(declaration.value().token()).ok()
            == Some(json!({"ready": scope}));
    if !covered {
        return Err(invalid(
            "readiness declaration does not cover the check parameters and clock",
        ));
    }
    let mut next = Some(
        member(&declaration, "manifest")
            .ok_or_else(|| invalid("readiness declaration requires a manifest"))?,
    );
    let mut seen = BTreeSet::new();
    let mut pending = BTreeSet::new();
    let mut memory = execution.workspace(|| 0)?;
    while let Some(id) = next {
        execution.work(1)?;
        if !seen.insert(id) {
            return Err(invalid("cyclic readiness manifest"));
        }
        memory.grow(96)?;
        let Some(link) = view.posit(id) else {
            pending.insert(id);
            break;
        };
        execution.trace_read(id)?;
        let delivered = member(&link, "delivery")
            .ok_or_else(|| invalid("manifest entry requires a delivery Posit"))?;
        next = member(&link, "previous");
        if link.appearance_set().appearances().len() != 1 + usize::from(next.is_some())
            || !has_string(&link, "included")
        {
            return Err(invalid("invalid readiness manifest entry"));
        }
        if view.posit(delivered).is_some() {
            execution.trace_read(delivered)?;
        } else {
            pending.insert(delivered);
        }
    }
    Ok(pending.into_iter().collect())
}
