# Benchmarks

Baseline captured before Step 10 JIT or snapshot optimizations. Step 10b adds
Cranelift JIT measurements for the hot `fib(40)` path. Step 13 adds COW heap
and incremental snapshot measurements for 10KB of dirty heap data.

Environment:

- OS: Linux 6.19.14-arch1-1 x86_64 GNU/Linux
- Rust: rustc 1.94.1 (e408947bf 2026-03-25)
- Command: `cargo bench --bench vm_benchmarks`
- Criterion: 0.5.1

| Benchmark | Before | After | Notes |
| --- | ---: | ---: | --- |
| `fib20_interpreter` | 2.4608 us | 2.7533 us | `programs::fibonacci(20)` through `VMState::run` |
| `snapshot_capture_1mb_heap` | 61.753 us | 61.112 us | Full snapshot still serializes 1MB heap |
| `snapshot_restore_1mb_heap` | 95.506 us | 180.33 us | Restore now rebuilds COW pages |
| `snapshot_delta_capture_10kb_dirty` | n/a | 864.66 ns | Captures only dirty COW pages |
| `snapshot_delta_apply_10kb_dirty` | n/a | 141.98 us | Applies dirty pages to base snapshot |
| `fib40_interpreter` | 1.1726 us | 1.1726 us | Execution-only run, VMState reused and reset |
| `fib40_jit_compiled` | 1.1726 us | 57.520 ns | Cached Cranelift machine code, 20.4x faster |

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

Step 13 raw Criterion ranges:

- `fib20_interpreter`: [2.5896 us, 2.7533 us, 2.9084 us]
- `snapshot_capture_1mb_heap`: [60.935 us, 61.112 us, 61.299 us]
- `snapshot_restore_1mb_heap`: [179.58 us, 180.33 us, 181.03 us]
- `snapshot_delta_capture_10kb_dirty`: [861.60 ns, 864.66 ns, 868.00 ns]
- `snapshot_delta_apply_10kb_dirty`: [141.35 us, 141.98 us, 142.70 us]
- `fib40_interpreter`: [1.1655 us, 1.1726 us, 1.1802 us]
- `fib40_jit_compiled`: [57.297 ns, 57.520 ns, 57.755 ns]

Step 13 transfer-size check:

- `incremental_snapshot_size_tracks_dirty_pages` modifies 10KB and asserts the
  serialized delta is under 20KB, while the full snapshot remains over 1MB.
