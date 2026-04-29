use aeon_vm::program::programs;
use aeon_vm::{ProgramStore, Snapshot, VMState};
use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};

fn bench_fib20_interpreter(c: &mut Criterion) {
    let program = programs::fibonacci(20);
    c.bench_function("fib20_interpreter", |b| {
        b.iter_batched(
            || VMState::new(&program),
            |mut state| black_box(state.run(black_box(&program)).unwrap()),
            BatchSize::SmallInput,
        )
    });
}

fn bench_snapshot_capture(c: &mut Criterion) {
    let program = programs::fibonacci(20);
    let state = VMState::new(&program);
    c.bench_function("snapshot_capture_1mb_heap", |b| {
        b.iter(|| black_box(Snapshot::capture(black_box(&state))))
    });
}

fn bench_snapshot_restore(c: &mut Criterion) {
    let program = programs::fibonacci(20);
    let store = ProgramStore::new();
    store.add(program.clone());
    let state = VMState::new(&program);
    let snapshot = Snapshot::capture(&state);

    c.bench_function("snapshot_restore_1mb_heap", |b| {
        b.iter(|| black_box(snapshot.restore(black_box(&store)).unwrap()))
    });
}

criterion_group!(
    benches,
    bench_fib20_interpreter,
    bench_snapshot_capture,
    bench_snapshot_restore
);
criterion_main!(benches);
