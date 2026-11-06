//! Incremental execution for statically total queries. Temporal patterns remain
//! blocking relations; nominal literal comparisons, effective evidence, and
//! mutation fallbacks retain the materialized executor's validation barriers.

use super::*;
use std::cmp::Ordering;
use std::collections::{BTreeSet, BinaryHeap};

type Visitor<'a> = dyn FnMut(Binding) -> Result<SinkFlow, DatabaseError> + 'a;

#[cfg(test)]
thread_local! {
    static SCANNED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) fn execute(
    database: &Database,
    query: &plan::PhysicalQuery,
    sink: &mut dyn RowSink,
    return_columns: &mut Option<Vec<String>>,
    execution: &ExecutionContext,
) -> Result<SearchOutcome, DatabaseError> {
    let has_return = !query.returns.is_empty();
    let mut outcome = SearchOutcome {
        matched_rows: 0,
        identity_bindings: HashMap::default(),
        has_return,
        sink_stopped: false,
        _identity_memory: Reservation::default(),
    };
    let sorted = !query.order.is_empty();
    if has_return && !sorted {
        *return_columns = Some(query.returns.clone());
        if let SinkFlow::Stop = sink.on_meta(&query.returns) {
            outcome.sink_stopped = true;
            return Ok(outcome);
        }
    }
    let mut seen = BTreeSet::new();
    let mut seen_memory = execution.workspace(|| 0)?;
    let mut ordering = OrderedRows::new(&query.order, query.limit, execution);
    let mut emit = |mut binding: Binding| {
        execution.work(1)?;
        // Union-private bindings cannot become correlations in an absence block.
        binding.0.retain(|name, _| query.domains.contains_key(name));
        for block in &query.not_exists {
            let mut found = false;
            visit_branch(database, block, 0, &binding, execution, &mut |_| {
                found = true;
                Ok(SinkFlow::Stop)
            })?;
            if found {
                return Ok(SinkFlow::Continue);
            }
        }
        outcome.matched_rows = outcome.matched_rows.saturating_add(1);
        if !has_return {
            return Ok(SinkFlow::Continue);
        }
        let _projection = execution.workspace(|| projection_bytes(&binding, &query.returns))?;
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
        if query.distinct {
            if seen.contains(&row) {
                return Ok(SinkFlow::Continue);
            }
            seen_memory.grow(row.heap_bytes().saturating_add(96))?;
            seen.insert(row.clone());
        }
        if sorted {
            ordering.push(binding, row)?;
        } else if let SinkFlow::Stop = sink.push(row) {
            outcome.sink_stopped = true;
            return Ok(SinkFlow::Stop);
        }
        Ok(SinkFlow::Continue)
    };
    for branch in &query.branches {
        if let SinkFlow::Stop = visit_branch(
            database,
            branch,
            0,
            &Binding::default(),
            execution,
            &mut emit,
        )? {
            break;
        }
    }
    if has_return && sorted {
        // Every sort key has been validated, including rows discarded by top-k.
        execution.check()?;
        let (rows, _ordering_memory) = ordering.finish()?;
        *return_columns = Some(query.returns.clone());
        if let SinkFlow::Stop = sink.on_meta(&query.returns) {
            outcome.sink_stopped = true;
            return Ok(outcome);
        }
        for row in rows {
            execution.check()?;
            if let SinkFlow::Stop = sink.push(row.row) {
                outcome.sink_stopped = true;
                break;
            }
        }
    }
    Ok(outcome)
}

fn visit_branch(
    database: &Database,
    branch: &plan::PlannedBranch,
    index: usize,
    input: &Binding,
    execution: &ExecutionContext,
    visitor: &mut Visitor<'_>,
) -> Result<SinkFlow, DatabaseError> {
    execution.work(1)?;
    let _input = execution.workspace(|| {
        input
            .heap_bytes()
            .saturating_add(size_of::<Binding>() as u64)
    })?;
    let Some(pattern) = branch.patterns.get(index) else {
        return visitor(input.clone());
    };
    visit_pattern(database, pattern, input, execution, &mut |binding| {
        for predicate in &pattern.predicates {
            execution.work(1)?;
            if !evaluate_predicate(predicate, &binding)? {
                return Ok(SinkFlow::Continue);
            }
        }
        visit_branch(database, branch, index + 1, &binding, execution, visitor)
    })
}

fn visit_pattern(
    database: &Database,
    planned: &plan::PlannedPattern,
    input: &Binding,
    execution: &ExecutionContext,
    visitor: &mut Visitor<'_>,
) -> Result<SinkFlow, DatabaseError> {
    let pattern = &planned.pattern;
    debug_assert!(pattern.effect.is_none());
    if pattern.cutoff.is_some() {
        // The entire history must participate in dominance before a row escapes.
        // This also keeps external field bindings on the correct side of reduction.
        for binding in evaluate_pattern(
            database,
            planned,
            input,
            &HashMap::new(),
            execution,
            &mut EffectCache::new(),
        )? {
            if let SinkFlow::Stop = visitor(binding)? {
                return Ok(SinkFlow::Stop);
            }
        }
        return Ok(SinkFlow::Continue);
    }
    let identities = candidate_identities(database, pattern, input, &planned.access, execution)?;
    let keeper = database.posit_keeper();
    for identity in identities {
        execution.work(1)?;
        #[cfg(test)]
        SCANNED.with(|count| count.set(count.get() + 1));
        // Release the lock before structural matching or invoking downstream
        // operators/sinks. Only the current candidate's Arc is kept alive here.
        let posit = keeper
            .lock()
            .map_err(|e| DatabaseError::Lock(e.to_string()))?
            .posit::<LiteralValue>(identity)
            .ok_or_else(|| {
                DatabaseError::Invariant(format!(
                    "posit index contains identity {identity} without a literal posit"
                ))
            })?;
        let _fields = execution.workspace(|| match &pattern.value {
            ValueSlot::Variable(_) | ValueSlot::LegacyVariable(_) => {
                posit.value().token().len() as u64 + 128
            }
            _ => 128,
        })?;
        let flow = visit_appearance_set(
            &pattern.appearances,
            &posit.appearance_set(),
            input,
            execution,
            &mut |binding| {
                if let Some(binding) = match_fields(pattern, &posit, binding)? {
                    visitor(binding)
                } else {
                    Ok(SinkFlow::Continue)
                }
            },
        )?;
        if let SinkFlow::Stop = flow {
            return Ok(SinkFlow::Stop);
        }
    }
    Ok(SinkFlow::Continue)
}

struct RankedRow<'a> {
    keys: Vec<SortKey>,
    row: Vec<ResultCell>,
    ordinal: usize,
    order: &'a [OrderItem],
    key_payload: u64,
}

impl Workspace for RankedRow<'_> {
    fn heap_bytes(&self) -> u64 {
        self.row
            .heap_bytes()
            .saturating_add(self.key_payload)
            .saturating_add(
                (self.keys.capacity() as u64).saturating_mul(size_of::<SortKey>() as u64),
            )
    }
}

impl Ord for RankedRow<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        for ((left, right), item) in self.keys.iter().zip(&other.keys).zip(self.order) {
            let comparison = match (left, right) {
                (SortKey::Value(left), SortKey::Value(right)) => order_value(left, right),
                (SortKey::Literal(left), SortKey::Literal(right)) => left.cmp(right),
                _ => unreachable!("sort domains were validated"),
            };
            if comparison != Ordering::Equal {
                return if item.descending {
                    comparison.reverse()
                } else {
                    comparison
                };
            }
        }
        self.ordinal.cmp(&other.ordinal)
    }
}

impl PartialOrd for RankedRow<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for RankedRow<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for RankedRow<'_> {}

enum SortKey {
    Value(BoundValue),
    Literal(crate::literal::LiteralOrderKey),
}

struct OrderedRows<'a> {
    order: &'a [OrderItem],
    capacity: Option<usize>,
    first_families: Vec<Option<LiteralFamily>>,
    ordinal: usize,
    heap: BinaryHeap<RankedRow<'a>>,
    all: BudgetVec<RankedRow<'a>>,
    heap_memory: Reservation,
    heap_payload: u64,
    execution: &'a ExecutionContext,
}

impl<'a> OrderedRows<'a> {
    fn new(order: &'a [OrderItem], limit: Option<usize>, execution: &'a ExecutionContext) -> Self {
        Self {
            order,
            // Keep a lookahead row for `limited`; usize::MAX needs no bound.
            capacity: limit.and_then(|limit| limit.checked_add(1)),
            first_families: vec![None; order.len()],
            ordinal: 0,
            heap: BinaryHeap::new(),
            all: BudgetVec::new(&execution.budget),
            heap_memory: execution.budget.memory(0).unwrap(),
            heap_payload: 0,
            execution,
        }
    }

    fn push(&mut self, binding: Binding, row: Vec<ResultCell>) -> Result<(), DatabaseError> {
        self.execution.work(self.order.len() as u64)?;
        let key_workspace = if self.execution.budget.tracks_memory() {
            self.order
                .iter()
                .map(|key| {
                    binding
                        .get(&key.variable)
                        .map_or(0, Workspace::heap_bytes)
                        .saturating_mul(4)
                        .saturating_add(256)
                })
                .fold(0, u64::saturating_add)
        } else {
            0
        };
        let _keys = self.execution.workspace(|| key_workspace)?;
        let mut keys = Vec::with_capacity(self.order.len());
        let mut key_payload = 0u64;
        for (item, first) in self.order.iter().zip(&mut self.first_families) {
            let value = binding
                .get(&item.variable)
                .ok_or_else(|| DatabaseError::UnknownVariable(item.variable.clone()))?;
            if let BoundValue::Literal(literal) = value {
                let key = literal.order_key().map_err(DatabaseError::Comparison)?;
                let family = literal.family();
                if let Some(first) = *first {
                    if (first == LiteralFamily::Certainty) != (family == LiteralFamily::Certainty) {
                        return Err(DatabaseError::Comparison(format!(
                            "ordering is unsupported between {first:?} and {family:?}"
                        )));
                    }
                } else {
                    *first = Some(family);
                }
                keys.push(SortKey::Literal(key));
                key_payload = key_payload.saturating_add(
                    (literal.token().len() as u64)
                        .saturating_mul(4)
                        .saturating_add(64),
                );
            } else {
                keys.push(SortKey::Value(value.clone()));
                key_payload = key_payload.saturating_add(value.heap_bytes());
            }
        }
        let ranked = RankedRow {
            keys,
            row,
            ordinal: self.ordinal,
            order: self.order,
            key_payload,
        };
        self.ordinal = self
            .ordinal
            .checked_add(1)
            .ok_or(DatabaseError::ResourceLimit {
                resource: "ordered query rows",
                limit: usize::MAX as u64,
            })?;
        if let Some(capacity) = self.capacity {
            self.execution.work(
                2 * (usize::BITS - self.heap.len().leading_zeros()) as u64
                    * self.order.len() as u64
                    + 1,
            )?;
            if self.heap.len() < capacity {
                if self.heap_memory.active() {
                    let reserved = if self.heap.len() == self.heap.capacity() {
                        self.heap.capacity().saturating_mul(2).max(4)
                    } else {
                        self.heap.capacity()
                    };
                    self.heap_payload = self.heap_payload.saturating_add(ranked.heap_bytes());
                    self.heap_memory.resize(
                        (reserved as u64)
                            .saturating_mul(size_of::<RankedRow>() as u64)
                            .saturating_add(self.heap_payload),
                    )?;
                    if reserved > self.heap.capacity() {
                        self.heap
                            .try_reserve_exact(reserved - self.heap.len())
                            .map_err(|_| {
                                DatabaseError::Execution("query workspace allocation failed".into())
                            })?;
                    }
                    self.heap_memory.resize(
                        (self.heap.capacity() as u64)
                            .saturating_mul(size_of::<RankedRow>() as u64)
                            .saturating_add(self.heap_payload),
                    )?;
                }
                self.heap.push(ranked);
            } else if ranked < *self.heap.peek().expect("top-k has a lookahead slot") {
                if self.heap_memory.active() {
                    self.heap_payload -= self.heap.peek().unwrap().heap_bytes();
                    self.heap_payload = self.heap_payload.saturating_add(ranked.heap_bytes());
                    self.heap_memory.resize(
                        (self.heap.capacity() as u64)
                            .saturating_mul(size_of::<RankedRow>() as u64)
                            .saturating_add(self.heap_payload),
                    )?;
                }
                *self.heap.peek_mut().unwrap() = ranked;
            }
        } else {
            self.all.push(ranked)?;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<(Vec<RankedRow<'a>>, Reservation), DatabaseError> {
        if self.capacity.is_some() {
            self.execution
                .work(sort_work(self.heap.len(), self.order.len()))?;
            Ok((self.heap.into_sorted_vec(), self.heap_memory))
        } else {
            self.execution
                .work(sort_work(self.all.len(), self.order.len()))?;
            let _scratch = self.execution.workspace(|| {
                (self.all.len() as u64).saturating_mul(size_of::<RankedRow>() as u64)
            })?;
            self.all.sort_by(Ord::cmp);
            Ok(self.all.into_parts())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::construct::PersistenceMode;

    #[derive(Default)]
    struct Rows(Vec<Vec<ResultCell>>);

    impl RowSink for Rows {
        fn push(&mut self, row: Vec<ResultCell>) -> SinkFlow {
            self.0.push(row);
            SinkFlow::Continue
        }
    }

    fn fixture() -> Database {
        let database = Database::new(PersistenceMode::InMemory).unwrap();
        let mut script = String::from("add role left, right, aux, extra; add posit ");
        for index in 0..12 {
            if index != 0 {
                script.push(',');
            }
            script.push_str(&format!(
                "[{{(+x_{index}, left)}}, {}, '{}'],[{{(x_{index}, right)}}, {index}, '2024']",
                index % 3,
                ["2023", "2024", "2024-06-01"][index % 3]
            ));
        }
        script.push(';');
        script.push_str("add posit [{(x_0, left)}, 7, '2025'], [{(x_0, right), (x_1, aux), (x_2, extra)}, 9, '2024'];");
        Engine::new(&database).execute(&script).unwrap();
        database
    }

    #[test]
    fn incremental_bags_and_row_order_match_materialized_reference() {
        let database = fixture();
        for body in [
            "[{(?x, left)}, ?v, *]",
            "[{(?x, left)}, ?v, *], [{(?y, right)}, ?w, *]",
            "[{(?x, left)}, ?v, *], [{(?x, right)}, ?w, *]",
            "[{(?x, left)}, ?v, *] union [{(?x, right)}, ?v, *]",
            "[{(?x, left)}, ?v, *] not exists { [{(?x, right)}, *, *], [{(*, left)}, *, *] }",
            "[{(?x, left)}, ?v, *] as of '2025'",
            "latest [{(?x, left)}, ?v, *] as of '2025'",
            "[{(*, left)}, ?v, *], [{(?x, left)}, ?v, *] as of '2025'",
            "[{(?x, left)}, ?v, ?t] as of '2025', [{(*, right)}, *, ?t]",
            "[{(?x, left)}, ?v, *], [{(?y, right)}, ?w, *] where ?v === 0",
            "[{(?x, left)}, ?v, *], [{(?y, right)}, ?w, *] where ?v === ?w",
            "[{(?x, left)}, ?v, ?t] where ?t < '2025'",
            "[{(?x, *), (*, *), ...}, ?v, *]",
        ] {
            for modifier in [
                "?v",
                "distinct ?v",
                "?v order by ?v",
                "distinct ?v order by ?v desc",
            ] {
                let script = format!("search {body} return {modifier};");
                let execution = ExecutionOptions::default().resolve(&script).unwrap();
                let compile = || {
                    let command = TraqulaParser::parse(Rule::traqula, &script)
                        .unwrap()
                        .next()
                        .unwrap();
                    plan::compile(&database, prepare(command, &execution).unwrap(), &execution)
                        .unwrap()
                };
                let planned = compile();
                assert!(planned.materialization_reason.is_none(), "{script}");
                let mut actual = Rows::default();
                execute(&database, &planned, &mut actual, &mut None, &execution).unwrap();
                let mut reference = Rows::default();
                execute_materialized(&database, compile(), &mut reference, &mut None, &execution)
                    .unwrap();
                assert_eq!(actual.0, reference.0, "{script}");
            }
        }
    }

    #[test]
    fn early_limits_and_absence_stop_scanning_without_materializing_joins() {
        let database = fixture();
        let engine = Engine::new(&database);
        SCANNED.with(|count| count.set(0));
        let result = engine
            .execute_collect(
                "search [{(?x, left)}, ?v, *], [{(?y, right)}, ?w, *] return ?v, ?w limit 1;",
            )
            .unwrap();
        assert_eq!(result.row_count, 1);
        assert!(result.limited);
        // One outer candidate and two inner candidates (one is lookahead).
        assert_eq!(SCANNED.with(|count| count.get()), 3);

        SCANNED.with(|count| count.set(0));
        let absent = engine
            .execute_collect(
                "search [{(?x, left)}, ?v, *] not exists { [{(*, right)}, *, *] } return ?v;",
            )
            .unwrap();
        assert_eq!(absent.row_count, 0);
        // Thirteen left posits; each absence needs only one right witness.
        assert_eq!(SCANNED.with(|count| count.get()), 26);

        struct Stop;
        impl RowSink for Stop {
            fn on_meta(&mut self, _: &[String]) -> SinkFlow {
                SinkFlow::Stop
            }
            fn push(&mut self, _: Vec<ResultCell>) -> SinkFlow {
                panic!("metadata stopped the query")
            }
        }
        SCANNED.with(|count| count.set(0));
        engine
            .execute_stream_single("search [{(?x, left)}, ?v, *] return ?v;", &mut Stop)
            .unwrap();
        assert_eq!(SCANNED.with(|count| count.get()), 0);
    }

    #[test]
    fn top_k_memory_is_bounded_and_checks_discarded_sort_values() {
        let order = [OrderItem {
            variable: "v".into(),
            descending: false,
        }];
        let execution = ExecutionOptions::default().resolve("").unwrap();
        let mut rows = OrderedRows::new(&order, Some(3), &execution);
        for i in 0..100 {
            let mut binding = Binding::default();
            binding
                .bind(
                    "v",
                    BoundValue::Literal(
                        LiteralValue::new(i.to_string(), LiteralFamily::Integer).unwrap(),
                    ),
                )
                .unwrap();
            rows.push(binding, Vec::new()).unwrap();
            assert!(rows.heap.len() <= 4);
            assert!(rows.all.is_empty());
        }
        let mut incompatible = Binding::default();
        incompatible
            .bind(
                "v",
                BoundValue::Literal(LiteralValue::new("100%", LiteralFamily::Certainty).unwrap()),
            )
            .unwrap();
        assert!(matches!(
            rows.push(incompatible, Vec::new()),
            Err(DatabaseError::Comparison(_))
        ));
        assert_eq!(rows.finish().unwrap().0.len(), 4);
    }
}
