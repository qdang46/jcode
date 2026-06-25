//! Benchmarks for todo operations.
//! Run with: cargo bench

use criterion::{Criterion, criterion_group, criterion_main};
use jcode_base::todo::{load_todos, save_todos};
use jcode_task_types::TodoItem;

fn bench_save_load(c: &mut Criterion) {
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("JCODE_HOME", tmp.path());
    let todos: Vec<_> = (0..50)
        .map(|i| TodoItem {
            content: format!("task {i}"),
            status: "pending".into(),
            ..Default::default()
        })
        .collect();

    c.bench_function("save_50_todos", |b| {
        b.iter(|| {
            save_todos("bench", &todos).unwrap();
        });
    });
    c.bench_function("load_50_todos", |b| {
        b.iter(|| load_todos("bench").unwrap());
    });
}

criterion_group!(benches, bench_save_load);
criterion_main!(benches);
