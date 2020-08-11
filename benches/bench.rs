use criterion::{criterion_group, criterion_main, Criterion};
use duct::cmd;

const BIN: &str = env!("CARGO_BIN_EXE_gitpr");

fn default() {
    cmd!(BIN).read().unwrap();
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("default", |b| b.iter(|| default()));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
