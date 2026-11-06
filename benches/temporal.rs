//! End-to-end temporal workloads. Fixtures and correctness checks are outside
//! the timed loop; the measurements include normal planning and result creation.

use criterion::{BenchmarkId, Criterion, Throughput};
use positorium::datatype::Time;
use positorium::{Database, EffectCut, Engine, PersistenceMode, TerrainOptions};
use std::cell::OnceCell;
use std::collections::HashSet;
use std::hint::black_box;

#[derive(Clone, Copy)]
enum Layout {
    Independent,
    History,
    Ties,
    Overlap,
    Skewed,
}

impl Layout {
    fn name(self) -> &'static str {
        match self {
            Self::Independent => "independent_sets",
            Self::History => "ordered_history",
            Self::Ties => "equal_times",
            Self::Overlap => "overlapping_precision",
            Self::Skewed => "skewed_history",
        }
    }

    fn entity(self, index: usize, rows: usize) -> usize {
        match self {
            Self::Independent => index,
            Self::Skewed if index >= rows * 7 / 8 => index + 1,
            _ => 0,
        }
    }

    fn time(self, index: usize) -> String {
        match self {
            Self::Independent | Self::Ties => "'2024-06-15'".into(),
            Self::Overlap => ["'2024'", "'2024-06'", "'2024-06-15'"][index % 3].into(),
            Self::History | Self::Skewed => timestamp(index, 1),
        }
    }

    fn maxima(self, rows: usize) -> usize {
        match self {
            Self::Independent | Self::Ties | Self::Overlap => rows,
            Self::History => 1,
            Self::Skewed => rows - rows * 7 / 8 + 1,
        }
    }
}

fn timestamp(index: usize, month: u32) -> String {
    let base = chrono::NaiveDate::from_ymd_opt(2024, month, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    format!(
        "'{}'",
        (base + chrono::Duration::seconds(index as i64)).format("%Y-%m-%d %H:%M:%S")
    )
}

fn history(layout: Layout, rows: usize) -> Database {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let mut script = String::from("add role status; add posit ");
    let mut entities = HashSet::new();
    for index in 0..rows {
        if index != 0 {
            script.push(',');
        }
        let entity = layout.entity(index, rows);
        let prefix = if entities.insert(entity) { "+" } else { "" };
        script.push_str(&format!(
            "[{{({prefix}entity_{entity}, status)}}, {index}, {}]",
            layout.time(index)
        ));
    }
    script.push(';');
    Engine::new(&database).execute(&script).unwrap();
    database
}

#[derive(Clone, Copy)]
enum Evidence {
    Targets(Layout),
    AssertionHistory,
    Retractions,
}

impl Evidence {
    fn name(self) -> &'static str {
        match self {
            Self::Targets(layout) => layout.name(),
            Self::AssertionHistory => "assertion_history",
            Self::Retractions => "older_retractions",
        }
    }

    fn maxima(self, rows: usize) -> usize {
        match self {
            Self::Targets(layout) => layout.maxima(rows),
            Self::AssertionHistory | Self::Retractions => 1,
        }
    }
}

fn evidence(layout: Evidence, rows: usize) -> Database {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let mut script = String::from("add role status;");
    match layout {
        Evidence::Targets(targets) => {
            let mut entities = HashSet::new();
            for index in 0..rows {
                let entity = targets.entity(index, rows);
                let prefix = if entities.insert(entity) { "+" } else { "" };
                let source = if index == 0 { "+source" } else { "source" };
                script.push_str(&format!(
                    "add posit +target_{index} [{{({prefix}entity_{entity}, status)}}, {index}, {}]; \
                     add posit [{{(target_{index}, posit), ({source}, ascertains)}}, 100%, '2025'];",
                    targets.time(index)
                ));
            }
        }
        Evidence::AssertionHistory | Evidence::Retractions => {
            script.push_str("add posit +target [{(+entity, status)}, 1, '2020']; add posit ");
            for index in 0..rows {
                if index != 0 {
                    script.push(',');
                }
                let source = if index == 0 { "+source" } else { "source" };
                // Old retractions do not remove later reassertions. This forces
                // an exhaustive retraction search in the pairwise baseline.
                let retraction = matches!(layout, Evidence::Retractions) && index < rows / 2;
                let certainty = if retraction { 0 } else { 100 };
                let month = if retraction { 1 } else { 2 };
                script.push_str(&format!(
                    "[{{(target, posit), ({source}, ascertains)}}, {certainty}%, {}]",
                    timestamp(index, month)
                ));
            }
            script.push(';');
        }
    }
    Engine::new(&database).execute(&script).unwrap();
    database
}

fn effect_cut() -> EffectCut {
    EffectCut::new(Time::new_end_of_time(), Time::new_end_of_time())
}

pub fn benchmarks(c: &mut Criterion) {
    let mut query = c.benchmark_group("temporal/query");
    for layout in [
        Layout::Independent,
        Layout::History,
        Layout::Ties,
        Layout::Overlap,
        Layout::Skewed,
    ] {
        for rows in [128, 512, 2_048] {
            let fixture = OnceCell::new();
            query.throughput(Throughput::Elements(rows as u64));
            query.bench_with_input(BenchmarkId::new(layout.name(), rows), &rows, |b, &rows| {
                let database = fixture.get_or_init(|| history(layout, rows));
                let engine = Engine::new(database);
                let script = "search ?p = [{(?entity, status)}, ?value, *] as of @EOT return ?p;";
                assert_eq!(
                    engine.execute_collect(script).unwrap().row_count,
                    layout.maxima(rows)
                );
                b.iter(|| black_box(engine.execute_collect(black_box(script)).unwrap().row_count));
            });
        }
    }
    for rows in [128, 512, 2_048] {
        let fixture = OnceCell::new();
        query.throughput(Throughput::Elements(rows as u64));
        query.bench_with_input(
            BenchmarkId::new("latest_history", rows),
            &rows,
            |b, &rows| {
                let database = fixture.get_or_init(|| history(Layout::History, rows));
                let engine = Engine::new(database);
                let script =
                    "search latest ?p = [{(?entity, status)}, ?value, *] as of @EOT return ?p;";
                assert_eq!(engine.execute_collect(script).unwrap().row_count, 1);
                b.iter(|| black_box(engine.execute_collect(black_box(script)).unwrap().row_count));
            },
        );
    }
    query.finish();

    let mut terrain = c.benchmark_group("temporal/terrain");
    for layout in [Layout::History, Layout::Ties, Layout::Overlap] {
        for rows in [128, 512, 2_048] {
            let fixture = OnceCell::new();
            terrain.throughput(Throughput::Elements(rows as u64));
            terrain.bench_with_input(BenchmarkId::new(layout.name(), rows), &rows, |b, &rows| {
                let database = fixture.get_or_init(|| history(layout, rows));
                let options = || TerrainOptions {
                    as_of: Some(Time::new_end_of_time()),
                    ..TerrainOptions::default()
                };
                assert_eq!(
                    database
                        .terrain_with_options(options())
                        .unwrap()
                        .frames
                        .current
                        .stats
                        .posits,
                    layout.maxima(rows) as u64
                );
                b.iter(|| black_box(database.terrain_with_options(options()).unwrap()));
            });
        }
    }
    terrain.finish();

    let mut effect = c.benchmark_group("temporal/effect");
    for layout in [
        Evidence::Targets(Layout::Independent),
        Evidence::Targets(Layout::History),
        Evidence::Targets(Layout::Ties),
        Evidence::Targets(Layout::Overlap),
        Evidence::AssertionHistory,
        Evidence::Retractions,
    ] {
        for rows in [128, 512, 2_048] {
            let fixture = OnceCell::new();
            effect.throughput(Throughput::Elements(rows as u64));
            effect.bench_with_input(BenchmarkId::new(layout.name(), rows), &rows, |b, &rows| {
                let database = fixture.get_or_init(|| evidence(layout, rows));
                let measured = database.information_in_effect(effect_cut()).unwrap();
                assert_eq!(measured.assertions().len(), layout.maxima(rows));
                eprintln!(
                    "temporal/effect/{}/{rows}: {} temporal comparisons, {} effective rows",
                    layout.name(),
                    measured.counters().temporal_comparisons,
                    measured.assertions().len()
                );
                b.iter(|| black_box(database.information_in_effect(effect_cut()).unwrap()));
            });
        }
    }
    effect.finish();
}
