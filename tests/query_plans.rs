use positorium::{
    Database, Engine, ExecutionOptions, ExecutionParameter, MultiStreamCallbacks, PersistenceMode,
    PlanAccess, PlanDomain, PlanNode, PlanOperator, QUERY_PLAN_VERSION, ResultCell, RowSink,
    SinkFlow,
};
use std::collections::HashMap;

fn fixture() -> Database {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    Engine::new(&db)
        .execute(
            "add role common, rare; add posit [{(+a, common)}, 1, '2024'], \
         [{(a, rare)}, 1, '2024'], [{(+b, common)}, 2, '2024'];",
        )
        .unwrap();
    db
}

fn walk<'a>(node: &'a PlanNode, out: &mut Vec<&'a PlanNode>) {
    out.push(node);
    for input in &node.inputs {
        walk(input, out);
    }
}

#[test]
fn explain_reports_typed_bindings_chosen_order_and_bound_bitmap_access() {
    let db = fixture();
    let engine = Engine::new(&db);
    let query = "search [{(?e, common)}, ?v, *], [{(?e, rare)}, *, *] return ?e, ?v;";
    let plan = engine.explain(query).unwrap();
    assert_eq!(plan.version, QUERY_PLAN_VERSION);
    assert_eq!(plan.root.bindings[0].domain, PlanDomain::Thing);
    assert_eq!(plan.root.bindings[1].domain, PlanDomain::Literal);
    let mut nodes = Vec::new();
    walk(&plan.root, &mut nodes);
    assert!(nodes.iter().any(|node| matches!(&node.operation,
        PlanOperator::NestedLoopJoin { pattern_order, .. } if pattern_order == &[2, 1])));
    assert!(nodes.iter().any(|node| matches!(&node.operation,
        PlanOperator::StructuralScan { source_pattern: 1, access: PlanAccess::Bitmap { roles, bindings }, .. }
        if roles == &["common"] && bindings == &["e"])));
    assert_eq!(engine.execute_collect(query).unwrap().row_count, 1);
    let reversed = "search [{(?e, rare)}, *, *], [{(?e, common)}, ?v, *] return ?e, ?v;";
    assert_eq!(
        engine.execute_collect(query).unwrap().rows,
        engine.execute_collect(reversed).unwrap().rows
    );
}

#[test]
fn explain_places_field_matching_on_the_correct_side_of_temporal_reduction() {
    let db = fixture();
    for (marker, expected_parent, expected_child) in
        [("", "fields", "maximal"), ("latest ", "maximal", "fields")]
    {
        let plan = Engine::new(&db)
            .explain(&format!(
                "search {marker}[{{(?e, common)}}, 1, *] as of '2025' return ?e;"
            ))
            .unwrap();
        let mut nodes = Vec::new();
        walk(&plan.root, &mut nodes);
        let label = |operation: &PlanOperator| match operation {
            PlanOperator::MatchFields { .. } => "fields",
            PlanOperator::MaximalByAppearanceSet { .. } => "maximal",
            _ => "other",
        };
        assert!(nodes.iter().any(|node| {
            label(&node.operation) == expected_parent
                && node
                    .inputs
                    .first()
                    .is_some_and(|child| label(&child.operation) == expected_child)
        }));

        let correlated = Engine::new(&db)
            .explain(&format!(
                "search [{{(*, rare)}}, ?v, *], \
             {marker}[{{(?e, common)}}, ?v, *] as of '2025' return ?e;"
            ))
            .unwrap();
        let mut nodes = Vec::new();
        walk(&correlated.root, &mut nodes);
        assert!(nodes.iter().any(|node| matches!(&node.operation,
            PlanOperator::JoinExternalFields { variables } if variables == &["v"])));
        let structural = nodes
            .iter()
            .find(|node| {
                matches!(
                    node.operation,
                    PlanOperator::StructuralScan {
                        source_pattern: 2,
                        ..
                    }
                )
            })
            .unwrap();
        assert!(
            !structural
                .bindings
                .iter()
                .any(|binding| binding.name == "v")
        );
    }
}

#[test]
fn explain_tracks_union_and_anti_join_scopes_through_result_modifiers() {
    let db = fixture();
    let engine = Engine::new(&db);
    let query = "search [{(?e, common)}, ?v, ?branch_time] \
        union ?branch_posit = [{(?e, common)}, ?v, *] \
        not exists { ?local = [{(?e, rare)}, *, *] } \
        where ?v >= 1 return distinct ?e order by ?e limit 1;";
    let plan = engine.explain(query).unwrap();
    let mut node = &plan.root;
    for expected in [
        "limit",
        "sort",
        "distinct",
        "project",
        "filter",
        "anti_join",
        "union_all",
    ] {
        assert_eq!(
            serde_json::to_value(&node.operation).unwrap()["operator"],
            expected
        );
        assert!(!node.bindings.iter().any(|binding| binding.name == "local"));
        if expected != "union_all" {
            node = &node.inputs[0];
        }
    }
    assert_eq!(
        node.bindings
            .iter()
            .map(|binding| binding.name.as_str())
            .collect::<Vec<_>>(),
        ["e", "v"]
    );
    let mut nodes = Vec::new();
    walk(&plan.root, &mut nodes);
    assert!(nodes.iter().any(|node| matches!(&node.operation,
        PlanOperator::AntiJoin { correlated } if correlated == &["e"])));
    let result = engine.execute_collect(query).unwrap();
    let expected = engine
        .execute_collect("search [{(?e, common)}, 2, *] return ?e;")
        .unwrap();
    assert_eq!(result.rows, expected.rows);
    assert!(!result.limited);
}

#[test]
fn explain_is_deterministic_with_resolved_parameters_and_clock() {
    let db = fixture();
    let options = || ExecutionOptions {
        now: Some(positorium::datatype::Time::new_year_from("2025").unwrap()),
        parameters: HashMap::from([(
            "target".into(),
            ExecutionParameter::Literal {
                text: "1.00".into(),
            },
        )]),
        max_rows_per_search: Some(5),
        ..ExecutionOptions::default()
    };
    let query = "search [{(?e, common)}, ?v, *] as of @NOW where ?v = $target return ?e limit 10;";
    let engine = Engine::new(&db);
    let first = engine.explain_with_options(query, options()).unwrap();
    let second = engine.explain_with_options(query, options()).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.resolved_now, "2025");
    assert!(matches!(
        first.root.operation,
        PlanOperator::Limit {
            rows: 5,
            presentation_only: true
        }
    ));
    let emitted = engine
        .execute_collect_with_options(&format!("explain {query}"), options())
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(emitted.rows[0][0].as_str()).unwrap(),
        serde_json::to_value(first).unwrap()
    );
}

#[test]
fn explain_never_evaluates_evidence_or_executes_fallback_or_script_mutations() {
    let db = fixture();
    let engine = Engine::new(&db);
    engine
        .execute("add posit [{(+dangling, posit), (+source, ascertains)}, 100%, '2024'];")
        .unwrap();
    let effect = "search [{(?e, common)}, ?v, *] in effect '2025', '2025' return ?e;";
    assert!(engine.execute_collect(effect).is_err());
    let plan = engine.explain(effect).unwrap();
    let mut nodes = Vec::new();
    walk(&plan.root, &mut nodes);
    assert!(
        nodes
            .iter()
            .any(|node| matches!(node.operation, PlanOperator::ResolveEffect { .. }))
    );

    let count = || {
        engine
            .execute_collect("search ?p = [*, *, *] return ?p;")
            .unwrap()
            .row_count
    };
    let before = count();
    let fallback =
        "search [{(?e, rare)}, 999, *] return ?e or add posit [{(+e, rare)}, 999, '2024'];";
    assert!(matches!(
        engine.explain(fallback).unwrap().root.operation,
        PlanOperator::SearchOrAdd { .. }
    ));
    assert_eq!(
        engine
            .execute_collect(&format!("explain {fallback}"))
            .unwrap()
            .row_count,
        1
    );
    assert_eq!(count(), before);
    assert!(
        engine
            .explain("add role forbidden; search [*, *, *];")
            .is_err()
    );
    assert!(!db.contains_role("forbidden").unwrap());
    assert!(engine.explain("search [{(?x, common)}, *, *] not exists { [{(?local, rare)}, *, *] } return ?local;").is_err());
}

#[derive(Default)]
struct Capture {
    starts: usize,
    ends: usize,
    rows: Vec<Vec<ResultCell>>,
}

impl RowSink for Capture {
    fn on_meta(&mut self, columns: &[String]) -> SinkFlow {
        assert_eq!(columns, &["plan"]);
        self.starts += 1;
        SinkFlow::Continue
    }
    fn push(&mut self, row: Vec<ResultCell>) -> SinkFlow {
        self.rows.push(row);
        SinkFlow::Continue
    }
}

impl MultiStreamCallbacks for Capture {
    fn on_result_set_start(&mut self, _: usize, columns: &[String], _: &str) {
        assert_eq!(columns, &["plan"]);
        self.starts += 1;
    }
    fn on_row(&mut self, _: usize, row: Vec<ResultCell>) -> bool {
        self.rows.push(row);
        true
    }
    fn on_result_set_end(&mut self, _: usize, rows: usize, limited: bool) {
        assert_eq!(rows, 1);
        assert!(!limited);
        self.ends += 1;
    }
}

#[test]
fn explain_uses_regular_result_framing_in_buffered_and_streaming_apis() {
    let db = fixture();
    let engine = Engine::new(&db);
    // The explained limit limits the described query, not its explanation.
    let query = "explain search [{(?e, common)}, ?v, *] return ?e limit 0;";
    let result = engine.execute_collect(query).unwrap();
    assert_eq!(result.columns, ["plan"]);
    assert_eq!(result.row_count, 1);
    assert!(!result.limited);
    let mut sink = Capture::default();
    assert_eq!(engine.execute_stream_single(query, &mut sink).unwrap().2, 1);
    assert_eq!(sink.starts, 1);
    assert_eq!(sink.rows.len(), 1);
    let multiple = format!("{query} explain search [*, *, *];");
    assert_eq!(engine.execute_collect_multi(&multiple).unwrap().len(), 2);
    let mut sink = Capture::default();
    engine.execute_stream_multi(&multiple, &mut sink).unwrap();
    assert_eq!((sink.starts, sink.ends, sink.rows.len()), (2, 2, 2));
    // The contextual keyword does not reserve existing mutation variable names.
    engine
        .execute("add posit [{(+explain, common)}, 3, '2024'];")
        .unwrap();
}
