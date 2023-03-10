use std::sync::Arc;

use criterion::{criterion_group, criterion_main, Criterion};
use milky_trie::Trie;
use rnglib::{Language, RNG};
use rocksdb::Options;

fn criterion_benchmark(c: &mut Criterion) {
    use rocksdb::DB;
    let path = "_path_for_rocksdb_storage";
    let _ = std::fs::remove_dir_all(path);

    let mut options = Options::default();
    options.increase_parallelism(4);
    options.create_if_missing(true);
    options.set_allow_mmap_reads(true);
    options.set_allow_mmap_writes(true);
    options.set_manual_wal_flush(true);

    let db = DB::open(&options, path).unwrap();
    let rng = RNG::new(&Language::Elven).unwrap();

    let mut t = Trie::new(Arc::new(db), "s");
    c.bench_function("milky_trie::insert", |b| {
        b.iter(|| {
            let name = rng.generate_name();
            t.insert(name, b"37");
        })
    });

    c.bench_function("milky_trie::get", |b| {
        b.iter(|| {
            let name = rng.generate_name();
            t.get(name);
        })
    });

    let mut t = qp_trie::Trie::new();
    c.bench_function("qp-trie::insert", |b| {
        b.iter(|| {
            let name = rng.generate_name();
            t.insert(name.into_bytes(), 37);
        })
    });

    c.bench_function("qp-trie::get", |b| {
        b.iter(|| {
            let name = rng.generate_name();
            t.get(name.as_bytes());
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
