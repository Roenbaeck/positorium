//! Parse the saved source and infer its persistent parameter and finding schema.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum FindingKind {
    Permits,
    Forbids,
    #[serde(rename = "violations")]
    Violation,
    Unresolved,
    Evidence,
}

pub(super) struct FindingQuery<'a> {
    search: Pair<'a, Rule>,
    pub(super) kind: FindingKind,
    pub(super) expected: Option<String>,
    pub(super) needs_readiness: bool,
}

// A parameter may have several compatible wire representations. Intersect its
// uses across all clauses before saving the definition, including empty feeds.
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum ParameterType {
    Integer,
    DateLiteral,
    OtherLiteral,
    Time,
    Thing,
    Posit,
}

type Signature = BTreeMap<String, BTreeSet<ParameterType>>;

fn parameter_use(signature: &mut Signature, name: &str, types: &[ParameterType]) -> Result<()> {
    let name = &name[1..];
    let allowed = signature
        .entry(name.into())
        .or_insert_with(|| types.iter().copied().collect());
    allowed.retain(|t| types.contains(t));
    if allowed.is_empty() {
        return Err(DatabaseError::Parameter(format!(
            "incompatible uses of ${name} in constraint"
        )));
    }
    Ok(())
}

fn parameter_types(
    part: Pair<'_, Rule>,
    domains: &HashMap<String, query::Domain>,
    signature: &mut Signature,
) -> Result<()> {
    use ParameterType::*;
    let rule = part.as_rule();
    if rule == Rule::condition {
        let mut inner = part.clone().into_inner();
        let left = inner.next().unwrap();
        if left.as_rule() == Rule::variable_reference {
            let _operator = inner.next();
            let rhs = inner.next().unwrap();
            if let Some(p) = rhs.into_inner().find(|p| p.as_rule() == Rule::parameter) {
                let types: &[_] = match domains[&left.as_str()[1..]] {
                    query::Domain::Literal => &[Integer, DateLiteral, OtherLiteral],
                    query::Domain::Time => &[Time],
                    query::Domain::Thing | query::Domain::Posit => &[Thing, Posit],
                    _ => &[],
                };
                return parameter_use(signature, p.as_str(), types);
            }
        }
    }
    for child in part.into_inner() {
        if child.as_rule() == Rule::parameter {
            let types: &[_] = match rule {
                Rule::appearance_time_search | Rule::effect_cut_operand | Rule::as_of_clause => {
                    &[Time]
                }
                Rule::appearing_value_search => &[OtherLiteral, Integer, DateLiteral, Time],
                Rule::appearance_search => &[Thing, Posit],
                Rule::day_count => &[Integer],
                Rule::date_operand => &[DateLiteral, Time],
                _ => {
                    return Err(DatabaseError::Invariant(format!(
                        "unexpected parameter position {rule:?}"
                    )));
                }
            };
            parameter_use(signature, child.as_str(), types)?;
        } else {
            parameter_types(child, domains, signature)?;
        }
    }
    Ok(())
}

pub(super) struct Definition<'a> {
    pub(super) queries: Vec<FindingQuery<'a>>,
    pub(super) parameters: BTreeSet<String>,
    pub(super) uses_now: bool,
}

impl<'a> Definition<'a> {
    pub(super) fn parse(source: &'a str) -> Result<Self> {
        let program = TraqulaParser::parse(Rule::constraint_program, source)
            .map_err(|e| invalid(format!("invalid read-only constraint program: {e}")))?
            .next()
            .unwrap();
        let mut queries = Vec::new();
        let mut parameters = BTreeSet::new();
        let mut uses_now = false;
        for part in program.into_inner().filter(|p| p.as_rule() != Rule::EOI) {
            let (kind, expected, search) = if part.as_rule() == Rule::constraint_finding {
                let mut inner = part.into_inner();
                let kind = match inner.next().unwrap().as_str().to_ascii_lowercase().as_str() {
                    "permits" => FindingKind::Permits,
                    "forbids" => FindingKind::Forbids,
                    "unresolved" => FindingKind::Unresolved,
                    "evidence" => FindingKind::Evidence,
                    _ => FindingKind::Violation,
                };
                (
                    kind,
                    Some(decoded(inner.next().unwrap().as_str())?),
                    inner.next().unwrap(),
                )
            } else {
                (FindingKind::Violation, None, part)
            };
            if fallback_clause(&search).is_some()
                || !search_has_return(&search)
                || search_limit(&search)?.is_some()
            {
                return Err(invalid(
                    "a constraint requires an exhaustive read-only search with RETURN and without LIMIT or a mutation fallback",
                ));
            }
            let nodes = search.clone().into_inner().flatten();
            parameters.extend(
                nodes
                    .clone()
                    .filter(|p| p.as_rule() == Rule::parameter)
                    .map(|p| p.as_str()[1..].to_owned()),
            );
            let needs_readiness = nodes.clone().any(|p| {
                matches!(
                    p.as_rule(),
                    Rule::not_exists_clause
                        | Rule::as_of_clause
                        | Rule::in_effect_clause
                        | Rule::latest_marker
                )
            });
            uses_now |= nodes
                .clone()
                .any(|p| p.as_rule() == Rule::constant && p.as_str() == "@NOW");
            queries.push(FindingQuery {
                search,
                kind,
                expected,
                needs_readiness,
            });
        }
        if queries
            .iter()
            .all(|q| matches!(q.kind, FindingKind::Evidence | FindingKind::Permits))
        {
            return Err(invalid("a constraint needs at least one finding search"));
        }
        Ok(Self {
            queries,
            parameters,
            uses_now,
        })
    }

    pub(super) fn prepare(
        &self,
        execution: &ExecutionContext,
    ) -> Result<Vec<query::PreparedQuery>> {
        self.queries
            .iter()
            .map(|q| {
                let prepared = query::prepare(q.search.clone(), execution)?;
                let subject = &prepared.returns()[0];
                if !matches!(
                    prepared.domains()[subject],
                    query::Domain::Thing | query::Domain::Posit
                ) {
                    return Err(invalid(
                        "the first constraint return column must identify a subject Thing or Posit",
                    ));
                }
                Ok(prepared)
            })
            .collect()
    }

    pub(super) fn metadata(&self, prepared: &[query::PreparedQuery]) -> Result<Value> {
        let mut signature = Signature::new();
        for (q, spec) in prepared.iter().zip(&self.queries) {
            let mut absence = 0;
            for clause in spec.search.clone().into_inner() {
                if clause.as_rule() == Rule::not_exists_clause {
                    parameter_types(clause, &q.absence_domains(absence)?, &mut signature)?;
                    absence += 1;
                } else {
                    parameter_types(clause, q.domains(), &mut signature)?;
                }
            }
        }
        for (name, kind) in [
            ("constraint_revision", ParameterType::Posit),
            ("constraint_policy", ParameterType::Thing),
        ] {
            if signature.contains_key(name) {
                parameter_use(&mut signature, &format!("${name}"), &[kind])?;
            }
        }
        let queries: Vec<_> = prepared
            .iter()
            .zip(&self.queries)
            .map(|(query, spec)| {
                let columns: Vec<_> = query
                    .returns()
                    .iter()
                    .map(|name| (name, query.domains()[name].label()))
                    .collect();
                json!({"columns": columns, "expected": spec.expected, "kind": spec.kind})
            })
            .collect();
        Ok(json!({
            "traqula": TRAQULA_VERSION,
            "constraint": CONSTRAINT_VERSION,
            "scope": "store",
            "queries": queries,
            "parameters": signature
        }))
    }

    pub(super) fn validate(&self, execution: &ExecutionContext) -> Result<Value> {
        let mut validation = execution.clone();
        validation.validating_definition = true;
        let placeholder = resolve_parameter(
            ExecutionParameter::Time {
                text: "@BOT".into(),
            },
            &execution.metadata.resolved_now,
        )?;
        for name in &self.parameters {
            validation
                .parameters
                .insert(name.clone(), placeholder.clone());
        }
        self.metadata(&self.prepare(&validation)?)
    }
}
