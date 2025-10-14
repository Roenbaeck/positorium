use positorium::traqula::ResultSet;
use criterion::{Criterion, criterion_group, criterion_main};
use std::time::Instant;

pub fn criterion_benchmark(c: &mut Criterion) {
    let start = Instant::now();
    let mut r1 = ResultSet::new();
    let r2 = ResultSet::new();
    println!("Time spent building the result sets: {:?}", start.elapsed());
    println!("Both result sets look like:");
    println!("{:?}", r1);
    c.bench_function("intersect 0", |b| b.iter(|| r1 &= &r2));
    c.bench_function("union 0", |b| b.iter(|| r1 |= &r2));
    c.bench_function("difference 0", |b| b.iter(|| r1 -= &r2));
    c.bench_function("symmetric difference 0", |b| b.iter(|| r1 ^= &r2));
    // -----------------------------------------------------------------------------------------------
    let start = Instant::now();
    let mut r1 = ResultSet::new();
    let mut r2 = ResultSet::new();
    r1.insert(42);
    r2.insert(42);
    println!("Time spent building the result sets: {:?}", start.elapsed());
    println!("Both result sets look like:");
    println!("{:?}", r1);
    c.bench_function("intersect 1", |b| b.iter(|| r1 &= &r2));
    c.bench_function("union 1", |b| b.iter(|| r1 |= &r2));
    c.bench_function("difference 1", |b| b.iter(|| r1 -= &r2));
    c.bench_function("symmetric difference 1", |b| b.iter(|| r1 ^= &r2));
    // -----------------------------------------------------------------------------------------------
    let start = Instant::now();
    let mut r1 = ResultSet::new();
    let mut r2 = ResultSet::new();
    for n in 1..1000 {
        r1.insert(n);
        r2.insert(n);
    }
    println!("Time spent building the result sets: {:?}", start.elapsed());
    println!("Both result sets look like:");
    println!("{:?}", r1);
    c.bench_function("intersect 1k", |b| b.iter(|| r1 &= &r2));
    c.bench_function("union 1k", |b| b.iter(|| r1 |= &r2));
    c.bench_function("difference 1k", |b| b.iter(|| r1 -= &r2));
    c.bench_function("symmetric difference 1k", |b| b.iter(|| r1 ^= &r2));
    // -----------------------------------------------------------------------------------------------
    let start = Instant::now();
    let mut r1 = ResultSet::new();
    let mut r2 = ResultSet::new();
    for n in 100000..200000 {
        r1.insert(n);
        r2.insert(n);
    }
    println!("Time spent building the result sets: {:?}", start.elapsed());
    println!("Both result sets look like:");
    println!("{:?}", r1);
    c.bench_function("intersect 100k", |b| b.iter(|| r1 &= &r2));
    c.bench_function("union 100k", |b| b.iter(|| r1 |= &r2));
    c.bench_function("difference 100k", |b| b.iter(|| r1 -= &r2));
    c.bench_function("symmetric difference 100k", |b| b.iter(|| r1 ^= &r2));
    // -----------------------------------------------------------------------------------------------
    let start = Instant::now();
    let mut r1 = ResultSet::new();
    let mut r2 = ResultSet::new();
    for n in 10000000..20000000 {
        r1.insert(n);
        r2.insert(n);
    }
    println!("Time spent building the result sets: {:?}", start.elapsed());
    println!("Both result sets look like:");
    println!("{:?}", r1);
    c.bench_function("intersect 10M", |b| b.iter(|| r1 &= &r2));
    c.bench_function("union 10M", |b| b.iter(|| r1 |= &r2));
    c.bench_function("difference 10M", |b| b.iter(|| r1 -= &r2));
    c.bench_function("symmetric difference 10M", |b| b.iter(|| r1 ^= &r2));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
