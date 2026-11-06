//! Planning and execution latency, including a sink that stops at its first row.
//! Identical fixtures and queries run against the materializing baseline.

use criterion::{BenchmarkId, Criterion};
use positorium::{
    Database, Engine, ExecutionOptions, PersistenceMode, QueryLimits, ResultCell, RowSink, SinkFlow,
};
use std::cell::OnceCell;
use std::hint::black_box;

fn fixture(rows: usize) -> Database {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let mut script = String::from("add role left, right; add posit ");
    for index in 0..rows {
        if index != 0 {
            script.push(',');
        }
        script.push_str(&format!(
            "[{{(+entity_{index}, left)}}, {index}, '2024'],\
             [{{(entity_{index}, right)}}, {index}, '2024']"
        ));
    }
    script.push(';');
    Engine::new(&database).execute(&script).unwrap();
    database
}

#[derive(Default)]
struct FirstRow(bool);

impl RowSink for FirstRow {
    fn push(&mut self, row: Vec<ResultCell>) -> SinkFlow {
        black_box(row);
        self.0 = true;
        SinkFlow::Stop
    }
}

pub fn benchmarks(c: &mut Criterion) {
    benchmark_limits(c, "incremental", QueryLimits::default());
    benchmark_limits(
        c,
        "query_budgets",
        QueryLimits {
            max_work_units: Some(20_000_000),
            max_memory_bytes: Some(256 * 1024 * 1024),
        },
    );
}

fn benchmark_limits(c: &mut Criterion, group: &str, query_limits: QueryLimits) {
    let mut group = c.benchmark_group(group);
    for (name, pattern, suffix, join, first_row) in [
        (
            "scan_limit",
            "[{(?x, left)}, ?v, *]",
            "return ?v limit 10",
            false,
            false,
        ),
        (
            "scan_first_row",
            "[{(?x, left)}, ?v, *]",
            "return ?v",
            false,
            true,
        ),
        (
            "scan_all",
            "[{(?x, left)}, ?v, *]",
            "return ?v",
            false,
            false,
        ),
        (
            "scan_top_k",
            "[{(?x, left)}, ?v, *]",
            "return ?v order by ?v desc limit 10",
            false,
            false,
        ),
        (
            "comparison_barrier",
            "[{(?x, left)}, ?v, *]",
            "where ?v >= 0 return ?v limit 10",
            false,
            false,
        ),
        (
            "join_limit",
            "[{(?x, left)}, ?v, *], [{(?y, right)}, ?w, *]",
            "return ?v, ?w limit 10",
            true,
            false,
        ),
        (
            "join_first_row",
            "[{(?x, left)}, ?v, *], [{(?y, right)}, ?w, *]",
            "return ?v, ?w",
            true,
            true,
        ),
        (
            "join_filter",
            "[{(?x, left)}, ?v, *], [{(?y, right)}, ?w, *]",
            "where ?v === 0 return ?v, ?w",
            true,
            false,
        ),
        (
            "join_distinct_limit",
            "[{(?x, left)}, ?v, *], [{(?y, right)}, ?w, *]",
            "return distinct ?v limit 10",
            true,
            false,
        ),
        (
            "join_top_k",
            "[{(?x, left)}, ?v, *], [{(?y, right)}, ?w, *]",
            "return ?v, ?w order by ?w desc, ?v limit 10",
            true,
            false,
        ),
        (
            "anti_join",
            "[{(?x, left)}, ?v, *] not exists { [{(*, right)}, *, *] }",
            "return ?v",
            true,
            false,
        ),
    ] {
        for rows in if join {
            [16, 64, 256]
        } else {
            [128, 512, 2_048]
        } {
            let database = OnceCell::new();
            let script = format!("search {pattern} {suffix};");
            group.bench_with_input(BenchmarkId::new(name, rows), &rows, |b, &rows| {
                let database = database.get_or_init(|| fixture(rows));
                let engine = Engine::new(database);
                let options = || ExecutionOptions {
                    query_limits,
                    ..ExecutionOptions::default()
                };
                let expected = match name {
                    "scan_all" | "join_filter" => rows,
                    "scan_first_row" | "join_first_row" => 1,
                    "anti_join" => 0,
                    _ => 10,
                };
                if first_row {
                    let mut sink = FirstRow::default();
                    engine
                        .execute_stream_single_with_options(&script, &mut sink, options())
                        .unwrap();
                    assert!(sink.0);
                    b.iter(|| {
                        let mut sink = FirstRow::default();
                        engine
                            .execute_stream_single_with_options(
                                black_box(&script),
                                &mut sink,
                                options(),
                            )
                            .unwrap();
                        black_box(sink.0)
                    });
                } else {
                    assert_eq!(
                        engine
                            .execute_collect_with_options(&script, options())
                            .unwrap()
                            .row_count,
                        expected,
                    );
                    b.iter(|| {
                        black_box(
                            engine
                                .execute_collect_with_options(black_box(&script), options())
                                .unwrap(),
                        )
                    });
                }
            });
        }
    }
    group.finish();
}
