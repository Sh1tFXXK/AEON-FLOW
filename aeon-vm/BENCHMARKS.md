# Benchmarks

Baseline captured before Step 10 JIT or snapshot optimizations. Step 10b adds
Cranelift JIT measurements for the hot `fib(40)` path.

Environment:

- OS: Linux 6.19.14-arch1-1 x86_64 GNU/Linux
- Rust: rustc 1.94.1 (e408947bf 2026-03-25)
- Command: `cargo bench --bench vm_benchmarks`
- Criterion: 0.5.1

| Benchmark | Before | After | Notes |
| --- | ---: | ---: | --- |
| `fib20_interpreter` | 2.4608 us | 2.9527 us | `programs::fibonacci(20)` through `VMState::run` |
| `snapshot_capture_1mb_heap` | 61.753 us | 65.235 us | `Snapshot::capture` clones 1MB heap |
| `snapshot_restore_1mb_heap` | 95.506 us | 102.40 us | Restore from snapshot through `ProgramStore` |
| `fib40_interpreter` | 1.2422 us | 1.2422 us | Execution-only run, VMState reused and reset |
| `fib40_jit_compiled` | 1.2422 us | 65.841 ns | Cached Cranelift machine code, 18.9x faster |

Before raw Criterion ranges:

- `fib20_interpreter`: [2.3054 us, 2.4608 us, 2.6226 us]
- `snapshot_capture_1mb_heap`: [61.079 us, 61.753 us, 62.501 us]
- `snapshot_restore_1mb_heap`: [94.431 us, 95.506 us, 96.840 us]

Step 10b raw Criterion ranges:

- `fib20_interpreter`: [2.7233 us, 2.9527 us, 3.2086 us]
- `snapshot_capture_1mb_heap`: [64.167 us, 65.235 us, 66.293 us]
- `snapshot_restore_1mb_heap`: [100.79 us, 102.40 us, 104.15 us]
- `fib40_interpreter`: [1.2298 us, 1.2422 us, 1.2540 us]
- `fib40_jit_compiled`: [65.292 ns, 65.841 ns, 66.434 ns]
