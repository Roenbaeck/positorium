use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use positorium::construct::{Database, PersistenceMode};
use positorium::traqula::{Engine, ResultSet};
use std::hint::black_box;
use std::path::{Path, PathBuf};
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

fn directory_bytes(path: &Path) -> u64 {
    std::fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().metadata().unwrap().len())
        .sum()
}

fn append_and_storage_benchmarks(c: &mut Criterion) {
    const POSITS: usize = 1_000;
    let script = posit_batch(POSITS);
    let mut group = c.benchmark_group("append");
    group.throughput(Throughput::Elements(POSITS as u64));
    group.bench_function("one_atomic_batch", |benchmark| {
        benchmark.iter_batched(
            || {
                let store = TemporaryStore::new("append");
                let database = Database::new(store.mode()).unwrap();
                Engine::new(&database).execute("add role value;").unwrap();
                (store, database)
            },
            |(store, database)| {
                Engine::new(&database).execute(black_box(&script)).unwrap();
                black_box(directory_bytes(&store.0));
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();

    let (store, bytes) = populated_file_store(POSITS);
    eprintln!(
        "positorium storage measurement: {POSITS} posits, {bytes} bytes, {:.2} bytes/posit",
        bytes as f64 / POSITS as f64
    );
    drop(store);
}

fn replay_and_query_benchmarks(c: &mut Criterion) {
    const POSITS: usize = 10_000;
    let (store, bytes) = populated_file_store(POSITS);
    let mut replay = c.benchmark_group("replay");
    replay.throughput(Throughput::Bytes(bytes));
    replay.bench_function(BenchmarkId::new("startup", POSITS), |benchmark| {
        benchmark.iter(|| {
            let database = Database::new(store.mode()).unwrap();
            black_box(database.role_count().unwrap());
        });
    });
    replay.finish();

    let database = populated_memory_database(POSITS);
    let engine = Engine::new(&database);
    let query = "search [{(*, value), ...}, ?value, *] return ?value order by ?value;";
    let mut query_group = c.benchmark_group("query");
    query_group.throughput(Throughput::Elements(POSITS as u64));
    query_group.bench_function(
        BenchmarkId::new("scan_order_project", POSITS),
        |benchmark| {
            benchmark.iter(|| {
                let result = engine.execute_collect(black_box(query)).unwrap();
                black_box(result.row_count);
            });
        },
    );
    query_group.finish();
}

fn result_set_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("result_set");
    for size in [1_u64, 1_000, 100_000, 1_000_000] {
        group.throughput(Throughput::Elements(size));
        group.bench_with_input(
            BenchmarkId::new("build", size),
            &size,
            |benchmark, &size| {
                benchmark.iter(|| {
                    let mut set = ResultSet::new();
                    for identity in 1..=size {
                        set.insert(black_box(identity));
                    }
                    black_box(set);
                });
            },
        );

        let mut measured = ResultSet::new();
        for identity in 1..=size {
            measured.insert(identity);
        }
        let heap_bytes = measured
            .multi
            .as_ref()
            .map_or(0, roaring::RoaringTreemap::serialized_size);
        eprintln!(
            "positorium ResultSet measurement: {size} identities, at least {} bytes",
            std::mem::size_of::<ResultSet>() + heap_bytes
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    append_and_storage_benchmarks,
    replay_and_query_benchmarks,
    result_set_benchmarks
);
criterion_main!(benches);
