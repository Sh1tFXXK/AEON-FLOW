use aeon_vm::program::programs;
use aeon_vm::{JitEngine, ProgramStore, Snapshot, VMState};
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

fn bench_fib40_interpreter(c: &mut Criterion) {
    let program = programs::fibonacci(40);
    let mut state = VMState::new(&program);
    c.bench_function("fib40_interpreter", |b| {
        b.iter(|| {
            reset_for_run(&mut state);
            black_box(state.run(black_box(&program)).unwrap());
            black_box(state.regs[2])
        })
    });
}

fn bench_fib40_jit_compiled(c: &mut Criterion) {
    let program = programs::fibonacci(40);
    let mut jit = JitEngine::new().unwrap();
    jit.compile(&program).unwrap();
    let program_id = program.id();
    let program_len = program.instructions.len();
    let mut state = VMState::new(&program);

    c.bench_function("fib40_jit_compiled", |b| {
        b.iter(|| {
            reset_for_run(&mut state);
            jit.run_compiled_cached(program_id, program_len, &mut state)
                .unwrap();
            black_box(state.regs[2])
        })
    });
}

fn reset_for_run(state: &mut VMState) {
    state.regs.fill(0);
    state.pc = 0;
    state.steps = 0;
    state.call_stack.clear();
}

criterion_group!(
    benches,
    bench_fib20_interpreter,
    bench_snapshot_capture,
    bench_snapshot_restore,
    bench_fib40_interpreter,
    bench_fib40_jit_compiled
);
criterion_main!(benches);
