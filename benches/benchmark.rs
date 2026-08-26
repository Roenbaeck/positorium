use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use positorium::construct::{Database, PersistenceMode};
use positorium::traqula::{Engine, ResultSet};
use std::cell::{Cell, OnceCell};
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_STORE: AtomicU64 = AtomicU64::new(1);

struct TemporaryStore(PathBuf);

impl TemporaryStore {
    fn new(label: &str) -> Self {
        let ordinal = NEXT_STORE.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "positorium-benchmark-{label}-{}-{ordinal}",
            std::process::id()
        )))
    }

    fn mode(&self) -> PersistenceMode {
        PersistenceMode::File(self.0.to_string_lossy().into_owned())
    }
}

impl Drop for TemporaryStore {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn posit_batch(count: usize) -> String {
    let mut script = String::from("add posit ");
    for index in 0..count {
        if index > 0 {
            script.push(',');
        }
        script.push_str(&format!(
            "[{{(+item_{index}, value)}}, {index}, '2024-01-01']"
        ));
    }
    script.push(';');
    script
}

fn mixed_role_batch(count: usize) -> String {
    let mut script = String::from("add posit ");
    let mut first = true;
    for index in 0..count {
        if !first {
            script.push(',');
        }
        first = false;
        script.push_str(&format!(
            "[{{(+entity_{index}, common)}}, {index}, '2024-01-01']"
        ));
        if index % 1_000 == 0 {
            script.push(',');
            script.push_str(&format!("[{{(entity_{index}, rare)}}, 1, '2024-01-01']"));
        }
    }
    script.push(';');
    script
}

fn historical_batch(entities: usize, versions: usize) -> String {
    let mut script = String::from("add posit ");
    let mut first = true;
    for entity in 0..entities {
        for version in 0..versions {
            if !first {
                script.push(',');
            }
            first = false;
            let binder = if version == 0 { "+" } else { "" };
            script.push_str(&format!(
                "[{{({binder}history_{entity}, status)}}, {version}, '{}-01-01']",
                2000 + version
            ));
        }
    }
    script.push(';');
    script
}

fn populated_file_store(count: usize) -> (TemporaryStore, u64) {
    let store = TemporaryStore::new("populated");
    let database = Database::new(store.mode()).unwrap();
    let engine = Engine::new(&database);
    engine.execute("add role value;").unwrap();
    engine.execute(&posit_batch(count)).unwrap();
    drop(database);
    let bytes = directory_bytes(&store.0);
    (store, bytes)
}

fn populated_memory_database(count: usize) -> Database {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    engine.execute("add role value;").unwrap();
    engine.execute(&posit_batch(count)).unwrap();
    database
}

fn mixed_role_database(count: usize) -> Database {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    engine.execute("add role common, rare;").unwrap();
    engine.execute(&mixed_role_batch(count)).unwrap();
    database
}

fn historical_database(entities: usize, versions: usize) -> Database {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    engine.execute("add role status;").unwrap();
    engine
        .execute(&historical_batch(entities, versions))
        .unwrap();
    database
}

fn directory_bytes(path: &Path) -> u64 {
    std::fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().metadata().unwrap().len())
        .sum()
}

fn append_and_storage_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("append/durable_atomic_batch");
    for posits in [1_usize, 10, 100, 1_000] {
        let script = posit_batch(posits);
        group.throughput(Throughput::Elements(posits as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(posits),
            &script,
            |benchmark, script| {
                benchmark.iter_batched(
                    || {
                        let store = TemporaryStore::new("append");
                        let database = Database::new(store.mode()).unwrap();
                        Engine::new(&database).execute("add role value;").unwrap();
                        (store, database)
                    },
                    |(_store, database)| {
                        Engine::new(&database).execute(black_box(script)).unwrap();
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

fn replay_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("replay/warm_startup");
    for posits in [10_000_usize, 100_000] {
        let fixture = OnceCell::new();
        let reported = Cell::new(false);
        group.throughput(Throughput::Elements(posits as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(posits),
            &posits,
            move |benchmark, &posits| {
                let (store, bytes) = fixture.get_or_init(|| populated_file_store(posits));
                if !reported.replace(true) {
                    eprintln!(
                        "positorium storage measurement: {posits} posits, {bytes} bytes, {:.2} bytes/posit",
                        *bytes as f64 / posits as f64
                    );
                }
                benchmark.iter(|| {
                    let database = Database::new(store.mode()).unwrap();
                    black_box(database.role_count().unwrap());
                });
            },
        );
    }
    group.finish();
}

fn query_benchmarks(c: &mut Criterion) {
    const POSITS: usize = 100_000;
    let full_database = OnceCell::new();
    let mut scan = c.benchmark_group("query/full_scan_order_project");
    scan.throughput(Throughput::Elements(POSITS as u64));
    scan.bench_function(BenchmarkId::from_parameter(POSITS), move |benchmark| {
        let engine = Engine::new(full_database.get_or_init(|| populated_memory_database(POSITS)));
        benchmark.iter(|| {
            let result = engine
                .execute_collect(black_box(
                    "search [{(*, value), ...}, ?value, *] return ?value order by ?value;",
                ))
                .unwrap();
            black_box(result.row_count);
        });
    });
    scan.finish();

    let mixed = Rc::new(OnceCell::new());
    let mut indexed = c.benchmark_group("query/indexed");
    indexed.throughput(Throughput::Elements(POSITS as u64));
    let selective_database = Rc::clone(&mixed);
    indexed.bench_function("selective_named_role", move |benchmark| {
        let mixed_engine =
            Engine::new(selective_database.get_or_init(|| mixed_role_database(POSITS)));
        benchmark.iter(|| {
            let result = mixed_engine
                .execute_collect(black_box(
                    "search [{(?entity, rare)}, ?value, *] return ?entity, ?value;",
                ))
                .unwrap();
            black_box(result.row_count);
        });
    });
    let join_database = Rc::clone(&mixed);
    indexed.bench_function("selective_natural_join", move |benchmark| {
        let mixed_engine = Engine::new(join_database.get_or_init(|| mixed_role_database(POSITS)));
        benchmark.iter(|| {
            let result = mixed_engine
                .execute_collect(black_box(
                    "search [{(?entity, rare)}, ?rare, *], [{(?entity, common)}, ?common, *] return ?entity, ?common;",
                ))
                .unwrap();
            black_box(result.row_count);
        });
    });
    let reversed_database = Rc::clone(&mixed);
    indexed.bench_function("selective_natural_join_reversed", move |benchmark| {
        let mixed_engine =
            Engine::new(reversed_database.get_or_init(|| mixed_role_database(POSITS)));
        benchmark.iter(|| {
            let result = mixed_engine
                .execute_collect(black_box(
                    "search [{(?entity, common)}, ?common, *], [{(?entity, rare)}, ?rare, *] return ?entity, ?common;",
                ))
                .unwrap();
            black_box(result.row_count);
        });
    });
    let absence_database = Rc::clone(&mixed);
    indexed.bench_function("correlated_not_exists", move |benchmark| {
        let mixed_engine =
            Engine::new(absence_database.get_or_init(|| mixed_role_database(POSITS)));
        benchmark.iter(|| {
            let result = mixed_engine
                .execute_collect(black_box(
                    "search [{(?entity, rare)}, ?rare, *] not exists { [{(?entity, common)}, *, *] } return ?entity;",
                ))
                .unwrap();
            black_box(result.row_count);
        });
    });
    indexed.finish();

    const ENTITIES: usize = 100;
    const VERSIONS: usize = 10;
    let historical = Rc::new(OnceCell::new());
    let mut history = c.benchmark_group("query/history");
    history.throughput(Throughput::Elements((ENTITIES * VERSIONS) as u64));
    let ordinary_database = Rc::clone(&historical);
    history.bench_function("ordinary_as_of", move |benchmark| {
        let historical_engine = Engine::new(
            ordinary_database.get_or_init(|| historical_database(ENTITIES, VERSIONS)),
        );
        benchmark.iter(|| {
            let result = historical_engine
                .execute_collect(black_box(
                    "search [{(?entity, status)}, ?status, ?time] as of '2099-01-01' return ?entity, ?status;",
                ))
                .unwrap();
            black_box(result.row_count);
        });
    });
    let latest_database = Rc::clone(&historical);
    history.bench_function("latest_matching_as_of", move |benchmark| {
        let historical_engine = Engine::new(
            latest_database.get_or_init(|| historical_database(ENTITIES, VERSIONS)),
        );
        benchmark.iter(|| {
            let result = historical_engine
                .execute_collect(black_box(
                    "search latest [{(?entity, status)}, ?status, ?time] as of '2099-01-01' return ?entity, ?status;",
                ))
                .unwrap();
            black_box(result.row_count);
        });
    });
    history.finish();
}

fn result_set_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("result_set/build");
    for size in [1_u64, 1_000, 100_000, 1_000_000] {
        let reported = Cell::new(false);
        group.throughput(Throughput::Elements(size));
        group.bench_with_input(
            BenchmarkId::new("dense", size),
            &size,
            |benchmark, &size| {
                if !reported.replace(true) {
                    let mut measured = ResultSet::new();
                    for identity in 1..=size {
                        measured.insert(identity);
                    }
                    report_result_set("dense", size, &measured);
                }
                benchmark.iter(|| {
                    let mut set = ResultSet::new();
                    for identity in 1..=size {
                        set.insert(black_box(identity));
                    }
                    black_box(set);
                });
            },
        );
    }

    for size in [1_000_u64, 100_000] {
        let reported = Cell::new(false);
        group.throughput(Throughput::Elements(size));
        group.bench_with_input(
            BenchmarkId::new("sparse", size),
            &size,
            |benchmark, &size| {
                if !reported.replace(true) {
                    let mut measured = ResultSet::new();
                    for index in 1..=size {
                        measured.insert(sparse_identity(index));
                    }
                    report_result_set("sparse", size, &measured);
                }
                benchmark.iter(|| {
                    let mut set = ResultSet::new();
                    for index in 1..=size {
                        set.insert(black_box(sparse_identity(index)));
                    }
                    black_box(set);
                });
            },
        );
    }
    group.finish();
}

fn sparse_identity(index: u64) -> u64 {
    index.wrapping_mul(1_000_003).rotate_left(17)
}

fn report_result_set(layout: &str, size: u64, measured: &ResultSet) {
    let heap_bytes = measured
        .multi
        .as_ref()
        .map_or(0, roaring::RoaringTreemap::serialized_size);
    eprintln!(
        "positorium ResultSet measurement: {layout}, {size} identities, at least {} bytes",
        std::mem::size_of::<ResultSet>() + heap_bytes
    );
}

criterion_group!(
    benches,
    append_and_storage_benchmarks,
    replay_benchmarks,
    query_benchmarks,
    result_set_benchmarks
);
criterion_main!(benches);
