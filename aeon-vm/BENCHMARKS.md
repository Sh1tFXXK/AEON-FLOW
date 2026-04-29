# Benchmarks

Baseline captured before Step 10 JIT or snapshot optimizations.

Environment:

- OS: Linux 6.19.14-arch1-1 x86_64 GNU/Linux
- Rust: rustc 1.94.1 (e408947bf 2026-03-25)
- Command: `cargo bench --bench vm_benchmarks`
- Criterion: 0.5.1

| Benchmark | Before | After | Notes |
| --- | ---: | ---: | --- |
| `fib20_interpreter` | 2.4608 us | pending | `programs::fibonacci(20)` through `VMState::run` |
| `snapshot_capture_1mb_heap` | 61.753 us | pending | `Snapshot::capture` clones 1MB heap |
| `snapshot_restore_1mb_heap` | 95.506 us | pending | Restore from snapshot through `ProgramStore` |

Raw Criterion ranges:

- `fib20_interpreter`: [2.3054 us, 2.4608 us, 2.6226 us]
- `snapshot_capture_1mb_heap`: [61.079 us, 61.753 us, 62.501 us]
- `snapshot_restore_1mb_heap`: [94.431 us, 95.506 us, 96.840 us]
