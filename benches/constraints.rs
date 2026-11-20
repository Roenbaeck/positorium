//! Compare a saved, set-based check with a reference scan over the same orders.
use criterion::{BenchmarkId, Criterion, Throughput};
use positorium::{Database, Engine, PersistenceMode};
use serde_json::Value;
use std::{fmt::Write, hint::black_box, time::Instant};

const FINDINGS: &str = "search ?action = [{(?order, cancelled)}, ?number, ?at],
    [{(?order, placed)}, *, ?day]
    not exists { [{(?order, approval)}, *, *] }
    where min_days_between(?day, ?at) > 30 return ?order, ?action, ?number;";

pub(super) fn benchmarks(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("constraints/sparse_approvals");
    for n in [1_000, 4_000] {
        let db = Database::new(PersistenceMode::InMemory).unwrap();
        let e = Engine::new(&db);
        e.execute("add role placed, cancelled, approval;").unwrap();
        let mut orders = Vec::new();
        for start in (0..n).step_by(100) {
            let mut batch = String::new();
            for id in start..(start + 100).min(n) {
                let late = id % 10 == 0;
                let approved = id % 40 == 0;
                let date = if late { "2024-02-01" } else { "2024-01-02" };
                write!(batch,"add posit [{{(+o{id},placed)}},{id},'2024-01-01'], [{{(o{id},cancelled)}},{id},'{date}']").unwrap();
                if approved {
                    write!(batch, ", [{{(o{id},approval)}},100%,'2024-01-31']").unwrap();
                }
                batch.push(';');
                orders.push((id, if late { 31 } else { 1 }, approved));
            }
            e.execute(&batch).unwrap();
        }
        e.execute(&format!(
            "add constraint \"Late cancellations\" at @BOT {{{FINDINGS}}};"
        ))
        .unwrap();
        let check = || {
            let result = e
                .execute_collect("check constraint \"Late cancellations\";")
                .unwrap();
            serde_json::from_str::<Value>(&result.rows[0][3].text).unwrap()
        };
        let reference = || {
            orders
                .iter()
                .filter_map(|&(id, days, approved)| (days > 30 && !approved).then_some(id))
                .collect::<Vec<_>>()
        };
        let start = Instant::now();
        let report = check();
        let elapsed = start.elapsed();
        let mut actual: Vec<usize> = report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["observed"][2]["text"].as_str().unwrap().parse().unwrap())
            .collect();
        actual.sort_unstable();
        assert_eq!(actual, reference());
        let plan = serde_json::to_string(&e.explain(FINDINGS).unwrap()).unwrap();
        assert!(plan.contains("anti_join") && plan.contains("bitmap"));
        eprintln!(
            "saved constraint: {n} orders, {} findings, {} work units, {elapsed:?}",
            actual.len(),
            report["work_units"]
        );
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("saved_check", n), &n, |b, _| {
            b.iter(|| black_box(check()))
        });
        group.bench_with_input(BenchmarkId::new("reference_scan", n), &n, |b, _| {
            b.iter(|| black_box(reference()))
        });
    }
    group.finish();
}
