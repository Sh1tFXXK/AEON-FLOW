// src/tests.rs

#[cfg(test)]
mod tests {
    use aeon_vm::asm::Assembler;
    use aeon_vm::forth::ForthPrototype;
    use aeon_vm::inst::Inst;
    use aeon_vm::program::{programs, Program};
    use aeon_vm::snapshot::Snapshot;
    use aeon_vm::store::ProgramStore;
    use aeon_vm::vm::{StepResult, VMError, VMState};

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn run_to_completion(program: &Program) -> VMState {
        let mut state = VMState::new(program);
        state.run(program).expect("run failed");
        state
    }

    fn append_store_bytes(
        insts: &mut Vec<Inst>,
        addr_reg: u8,
        value_reg: u8,
        one_reg: u8,
        start_addr: u64,
        bytes: &[u8],
    ) {
        insts.push(Inst::LoadImm {
            dst: addr_reg,
            val: start_addr,
        });
        for &byte in bytes {
            insts.push(Inst::LoadImm {
                dst: value_reg,
                val: byte as u64,
            });
            insts.push(Inst::StoreMem {
                addr: addr_reg,
                src: value_reg,
            });
            insts.push(Inst::Add {
                dst: addr_reg,
                a: addr_reg,
                b: one_reg,
            });
        }
    }

    fn vfs_roundtrip_program() -> (Program, usize, usize) {
        let path = b"test.txt";
        let data = b"hello";
        let path_addr = 0;
        let data_addr = 32;
        let read_addr = 64;

        let mut insts = vec![Inst::LoadImm { dst: 9, val: 1 }];
        append_store_bytes(&mut insts, 7, 8, 9, path_addr, path);
        append_store_bytes(&mut insts, 7, 8, 9, data_addr, data);

        insts.extend([
            Inst::LoadImm {
                dst: 1,
                val: path_addr,
            },
            Inst::LoadImm {
                dst: 2,
                val: path.len() as u64,
            },
            Inst::LoadImm { dst: 3, val: 1 },
            Inst::Syscall { num: 0 },
            Inst::Mov { dst: 4, src: 0 },
            Inst::Mov { dst: 1, src: 4 },
            Inst::LoadImm {
                dst: 2,
                val: data_addr,
            },
            Inst::LoadImm {
                dst: 3,
                val: data.len() as u64,
            },
            Inst::Syscall { num: 2 },
            Inst::Mov { dst: 1, src: 4 },
            Inst::Syscall { num: 3 },
        ]);

        let snap_after_write = insts.len();

        insts.extend([
            Inst::LoadImm {
                dst: 1,
                val: path_addr,
            },
            Inst::LoadImm {
                dst: 2,
                val: path.len() as u64,
            },
            Inst::LoadImm { dst: 3, val: 0 },
            Inst::Syscall { num: 0 },
            Inst::Mov { dst: 5, src: 0 },
            Inst::Mov { dst: 1, src: 5 },
            Inst::LoadImm {
                dst: 2,
                val: read_addr,
            },
            Inst::LoadImm {
                dst: 3,
                val: data.len() as u64,
            },
            Inst::Syscall { num: 1 },
            Inst::Mov { dst: 6, src: 0 },
            Inst::Mov { dst: 1, src: 5 },
            Inst::Syscall { num: 3 },
            Inst::LoadImm {
                dst: 7,
                val: read_addr,
            },
            Inst::LoadMem { dst: 10, addr: 7 },
            Inst::Add { dst: 7, a: 7, b: 9 },
            Inst::LoadMem { dst: 11, addr: 7 },
            Inst::Add { dst: 7, a: 7, b: 9 },
            Inst::LoadMem { dst: 12, addr: 7 },
            Inst::Add { dst: 7, a: 7, b: 9 },
            Inst::LoadMem { dst: 13, addr: 7 },
            Inst::Add { dst: 7, a: 7, b: 9 },
            Inst::LoadMem { dst: 14, addr: 7 },
            Inst::Halt,
        ]);

        (Program::new(insts), snap_after_write, read_addr as usize)
    }

    fn forth_fib_source() -> &'static str {
        r#"
: fib
  var n set n
  0 var a set a
  1 var b set b
  get n 0 do
    get b get a get b + set b set a
  loop
  get a
;

10 fib .
"#
    }

    // ── VM instruction correctness ────────────────────────────────────────────

    #[test]
    fn load_imm() {
        let p = Program::new(vec![Inst::LoadImm { dst: 5, val: 999 }, Inst::Halt]);
        let state = run_to_completion(&p);
        assert_eq!(state.regs[5], 999);
    }

    #[test]
    fn mov() {
        let p = Program::new(vec![
            Inst::LoadImm { dst: 0, val: 42 },
            Inst::Mov { dst: 1, src: 0 },
            Inst::Halt,
        ]);
        let state = run_to_completion(&p);
        assert_eq!(state.regs[1], 42);
        assert_eq!(state.regs[0], 42); // src unchanged
    }

    #[test]
    fn add_wrapping() {
        let p = Program::new(vec![
            Inst::LoadImm {
                dst: 0,
                val: u64::MAX,
            },
            Inst::LoadImm { dst: 1, val: 1 },
            Inst::Add { dst: 2, a: 0, b: 1 },
            Inst::Halt,
        ]);
        let state = run_to_completion(&p);
        assert_eq!(state.regs[2], 0); // wraps to zero
    }

    #[test]
    fn sub_wrapping() {
        let p = Program::new(vec![
            Inst::LoadImm { dst: 0, val: 0 },
            Inst::LoadImm { dst: 1, val: 1 },
            Inst::Sub { dst: 2, a: 0, b: 1 },
            Inst::Halt,
        ]);
        let state = run_to_completion(&p);
        assert_eq!(state.regs[2], u64::MAX);
    }

    #[test]
    fn mul() {
        let p = Program::new(vec![
            Inst::LoadImm { dst: 0, val: 6 },
            Inst::LoadImm { dst: 1, val: 7 },
            Inst::Mul { dst: 2, a: 0, b: 1 },
            Inst::Halt,
        ]);
        let state = run_to_completion(&p);
        assert_eq!(state.regs[2], 42);
    }

    #[test]
    fn jz_taken_and_not_taken() {
        // Taken
        let p = Program::new(vec![
            Inst::LoadImm { dst: 0, val: 0 },  // r0 = 0
            Inst::Jz { cond: 0, off: 2 },      // r0==0 → jump to pc=3
            Inst::LoadImm { dst: 1, val: 99 }, // skipped
            Inst::LoadImm { dst: 1, val: 42 }, // runs
            Inst::Halt,
        ]);
        assert_eq!(run_to_completion(&p).regs[1], 42);

        // Not taken
        let p2 = Program::new(vec![
            Inst::LoadImm { dst: 0, val: 1 },  // r0 = 1
            Inst::Jz { cond: 0, off: 2 },      // not taken
            Inst::LoadImm { dst: 1, val: 42 }, // runs
            Inst::LoadImm { dst: 1, val: 99 }, // also runs
            Inst::Halt,
        ]);
        assert_eq!(run_to_completion(&p2).regs[1], 99);
    }

    #[test]
    fn call_and_ret() {
        let p = Program::new(vec![
            // pc=0: main
            Inst::Call { addr: 3 },           // call subroutine at 3
            Inst::LoadImm { dst: 0, val: 1 }, // runs after ret (pc=2... wait, pc=1)
            Inst::Halt,
            // pc=3: subroutine
            Inst::LoadImm { dst: 1, val: 77 },
            Inst::Ret,
        ]);
        let state = run_to_completion(&p);
        assert_eq!(state.regs[1], 77); // subroutine ran
        assert_eq!(state.regs[0], 1); // code after call ran
    }

    #[test]
    fn ret_on_empty_stack_is_error() {
        let p = Program::new(vec![Inst::Ret]);
        let mut state = VMState::new(&p);
        assert_eq!(state.step(&p), StepResult::Error(VMError::EmptyCallStack));
    }

    // ── Programs ──────────────────────────────────────────────────────────────

    #[test]
    fn fibonacci_known_values() {
        let expected = [0u64, 1, 1, 2, 3, 5, 8, 13, 21, 34, 55];
        for (n, &exp) in expected.iter().enumerate().skip(1) {
            let p = programs::fibonacci(n as u64);
            let state = run_to_completion(&p);
            assert_eq!(state.regs[2], exp, "fib({}) should be {}", n, exp);
        }
    }

    #[test]
    fn factorial_known_values() {
        let cases: &[(u64, u64)] = &[(1, 1), (5, 120), (7, 5040), (10, 3628800)];
        for &(n, exp) in cases {
            let p = programs::factorial(n);
            let state = run_to_completion(&p);
            assert_eq!(state.regs[1], exp, "{}! should be {}", n, exp);
        }
    }

    // ── Program identity ──────────────────────────────────────────────────────

    #[test]
    fn same_instructions_same_id() {
        let p1 = programs::fibonacci(10);
        let p2 = programs::fibonacci(10);
        assert_eq!(p1.id(), p2.id());
    }

    #[test]
    fn different_instructions_different_id() {
        let p1 = programs::fibonacci(10);
        let p2 = programs::factorial(10);
        assert_ne!(p1.id(), p2.id());
    }

    #[test]
    fn metadata_does_not_affect_id() {
        let mut p1 = programs::fibonacci(10);
        let mut p2 = programs::fibonacci(10);
        p1.metadata.name = "foo".into();
        p2.metadata.name = "bar".into();
        // Same instructions → same ID, even with different names
        assert_eq!(p1.id(), p2.id());
    }

    // ── Snapshot correctness ──────────────────────────────────────────────────

    #[test]
    fn snapshot_does_not_contain_instructions() {
        // Core invariant: snapshot size must be independent of program size.
        // We verify this by comparing snapshots from a 10-instruction program
        // vs a 1000-instruction program (padded with Halt). If the snapshot
        // contained instructions, the sizes would differ.

        let store = ProgramStore::new();

        // Small program
        let small = programs::fibonacci(5);
        let _small_id = store.add(small.clone());
        let mut state_small = VMState::new(&small);
        state_small.run_bounded(&small, 3);
        let snap_small = Snapshot::capture(&state_small);

        // Large program: same fib but padded with many extra Halts
        // (extra instructions don't change the path, just program size)
        let mut large_code = programs::fibonacci(5).instructions;
        for _ in 0..1000 {
            large_code.push(Inst::Halt); // unreachable padding
        }
        let large = Program::new(large_code);
        let _large_id = store.add(large.clone());
        let mut state_large = VMState::new(&large);
        state_large.run_bounded(&large, 3);
        let snap_large = Snapshot::capture(&state_large);

        // Snapshot sizes should differ only by ProgramId (32 bytes) content,
        // not by instruction count. Both snapshots are the same size.
        assert_eq!(
            snap_small.byte_size(),
            snap_large.byte_size(),
            "snapshot size must not depend on program length"
        );
    }

    #[test]
    fn snapshot_roundtrip() {
        let store = ProgramStore::new();
        let p = programs::fibonacci(10);
        store.add(p.clone());

        let mut state = VMState::new(&p);
        state.run_bounded(&p, 15);

        let snap = Snapshot::capture(&state);
        let state2 = snap.restore(&store).expect("restore failed");

        assert_eq!(state.regs, state2.regs);
        assert_eq!(state.pc, state2.pc);
        assert_eq!(state.call_stack, state2.call_stack);
        assert_eq!(state.steps, state2.steps);
    }

    #[test]
    fn snapshot_mid_execution_gives_correct_result() {
        // Uninterrupted run
        let p = programs::fibonacci(10);
        let state_a = run_to_completion(&p);

        // Interrupted at step 15, snapshot, restore, continue
        let store = ProgramStore::new();
        store.add(p.clone());

        let mut state_b = VMState::new(&p);
        state_b.run_bounded(&p, 15);
        let snap = Snapshot::capture(&state_b);
        let mut state_b2 = snap.restore(&store).unwrap();
        state_b2.run(&p).unwrap();

        assert_eq!(state_a.regs[2], state_b2.regs[2]);
        assert_eq!(state_a.regs[2], 55);
    }

    #[test]
    fn snapshot_at_every_step_gives_correct_result() {
        let p = programs::fibonacci(8);
        let expected = run_to_completion(&p).regs[2];

        // Count total steps
        let total_steps = run_to_completion(&p).steps as usize;

        let store = ProgramStore::new();
        store.add(p.clone());

        for snap_at in 1..total_steps {
            let mut state = VMState::new(&p);
            let (_, halted) = state.run_bounded(&p, snap_at);
            if halted {
                break;
            }

            let snap = Snapshot::capture(&state);
            let mut state2 = snap.restore(&store).unwrap();
            state2.run(&p).unwrap();

            assert_eq!(
                state2.regs[2], expected,
                "snapshot at step {} gives wrong result",
                snap_at
            );
        }
    }

    #[test]
    fn restore_fails_without_program() {
        let store = ProgramStore::new();
        let p = programs::fibonacci(5);
        // Note: NOT added to store

        let state = VMState::new(&p);
        let snap = Snapshot::capture(&state);

        let result = snap.restore(&store);
        assert!(result.is_err(), "should fail when program not in store");
    }

    #[test]
    fn snapshot_serialization_roundtrip() {
        let p = programs::fibonacci(10);
        let store = ProgramStore::new();
        store.add(p.clone());

        let mut state = VMState::new(&p);
        state.run_bounded(&p, 10);

        let snap = Snapshot::capture(&state);
        let bytes = snap.to_bytes();
        let snap2 = Snapshot::from_bytes(&bytes).expect("deserialize failed");
        let state2 = snap2.restore(&store).unwrap();

        assert_eq!(state.regs, state2.regs);
        assert_eq!(state.pc, state2.pc);
    }

    #[test]
    fn snapshot_format_version_is_current() {
        let p = programs::fibonacci(5);
        let state = VMState::new(&p);
        let snap = Snapshot::capture(&state);

        assert_eq!(snap.format_version, 1);
    }

    #[test]
    fn snapshot_rejects_unsupported_format_version() {
        let p = programs::fibonacci(5);
        let state = VMState::new(&p);
        let mut snap = Snapshot::capture(&state);
        snap.format_version = 999;
        let bytes = snap.to_bytes();

        let err = Snapshot::from_bytes(&bytes).unwrap_err().to_string();
        assert!(
            err.contains("unsupported snapshot format"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn corrupt_snapshot_returns_error() {
        let p = programs::fibonacci(5);
        let state = VMState::new(&p);
        let snap = Snapshot::capture(&state);
        let mut bytes = snap.to_bytes();
        bytes[0] = bytes[0].wrapping_add(1);

        assert!(Snapshot::from_bytes(&bytes).is_err());
    }

    #[test]
    fn heap_alloc_write_read() {
        let src = r#"
load r0, 10
alloc r1, r0
load r2, 42
storemem r1, r2
loadmem r3, r1
halt
"#;
        let p = Assembler::new().assemble(src).unwrap();
        let state = run_to_completion(&p);

        assert_eq!(state.regs[1], 0);
        assert_eq!(state.regs[3], 42);
        assert_eq!(state.heap[0], 42);
        assert_eq!(state.heap_top, 10);
    }

    #[test]
    fn heap_survives_snapshot() {
        let p = Program::new(vec![
            Inst::LoadImm { dst: 0, val: 10 },
            Inst::Alloc { dst: 1, size: 0 },
            Inst::LoadImm { dst: 2, val: 42 },
            Inst::StoreMem { addr: 1, src: 2 },
            Inst::LoadMem { dst: 3, addr: 1 },
            Inst::Halt,
        ]);
        let store = ProgramStore::new();
        store.add(p.clone());

        let mut state = VMState::new(&p);
        state.run_bounded(&p, 4);
        let snap = Snapshot::capture(&state);
        let mut restored = snap.restore(&store).unwrap();
        restored.run(&p).unwrap();

        assert_eq!(restored.heap[0], 42);
        assert_eq!(restored.heap_top, 10);
        assert_eq!(restored.regs[3], 42);
    }

    #[test]
    fn out_of_bounds_returns_error() {
        let p = Program::new(vec![
            Inst::LoadImm {
                dst: 0,
                val: 1024 * 1024,
            },
            Inst::LoadMem { dst: 1, addr: 0 },
            Inst::Halt,
        ]);
        let mut state = VMState::new(&p);

        assert_eq!(state.step(&p), StepResult::Ok);
        match state.step(&p) {
            StepResult::Error(VMError::MemoryOutOfBounds { addr, heap_len }) => {
                assert_eq!(addr, 1024 * 1024);
                assert_eq!(heap_len, 1024 * 1024);
            }
            other => panic!("expected MemoryOutOfBounds, got {:?}", other),
        }
    }

    #[test]
    fn vfs_write_read_roundtrip() {
        let (program, _, read_addr) = vfs_roundtrip_program();
        let state = run_to_completion(&program);

        assert_eq!(state.regs[6], 5);
        assert_eq!(&state.heap[read_addr..read_addr + 5], b"hello");
        assert_eq!(
            [
                state.regs[10],
                state.regs[11],
                state.regs[12],
                state.regs[13],
                state.regs[14],
            ],
            [
                b'h' as u64,
                b'e' as u64,
                b'l' as u64,
                b'l' as u64,
                b'o' as u64
            ]
        );
    }

    #[test]
    fn vfs_survives_snapshot() {
        let (program, snap_at, read_addr) = vfs_roundtrip_program();
        let store = ProgramStore::new();
        store.add(program.clone());

        let mut state = VMState::new(&program);
        let (_, halted) = state.run_bounded(&program, snap_at);
        assert!(!halted);

        let snap = Snapshot::capture(&state);
        let mut restored = snap.restore(&store).unwrap();
        restored.run(&program).unwrap();

        assert_eq!(restored.regs[6], 5);
        assert_eq!(&restored.heap[read_addr..read_addr + 5], b"hello");
    }

    #[test]
    fn forth_fib10_outputs_55() {
        let program = Program::new(vec![Inst::Halt]);
        let mut state = VMState::new(&program);

        ForthPrototype::start(&mut state, forth_fib_source()).unwrap();
        let output = ForthPrototype::run(&mut state).unwrap();

        assert_eq!(output, vec![55]);
        assert_eq!(state.regs[200], aeon_vm::forth::STACK_BASE as u64);
    }

    #[test]
    fn forth_state_survives_snapshot() {
        let program = Program::new(vec![Inst::Halt]);
        let store = ProgramStore::new();
        store.add(program.clone());

        let mut state = VMState::new(&program);
        ForthPrototype::start(&mut state, forth_fib_source()).unwrap();
        let halted = ForthPrototype::run_steps(&mut state, 12).unwrap();
        assert!(!halted);

        let snap = Snapshot::capture(&state);
        let mut restored = snap.restore(&store).unwrap();
        let output = ForthPrototype::run(&mut restored).unwrap();

        assert_eq!(output, vec![55]);
        assert_eq!(restored.regs[200], aeon_vm::forth::STACK_BASE as u64);
    }

    #[test]
    fn forth_core_language_features() {
        let program = Program::new(vec![Inst::Halt]);
        let mut state = VMState::new(&program);
        let src = "6 7 * 5 - . 0 if 99 . then 1 if 3 4 + . then";

        ForthPrototype::start(&mut state, src).unwrap();
        let output = ForthPrototype::run(&mut state).unwrap();

        assert_eq!(output, vec![37, 7]);
    }

    // ── ProgramStore ──────────────────────────────────────────────────────────

    #[test]
    fn store_is_idempotent() {
        let store = ProgramStore::new();
        let p = programs::fibonacci(5);
        let id1 = store.add(p.clone());
        let id2 = store.add(p.clone());
        assert_eq!(id1, id2);
        assert_eq!(store.count(), 1); // not two copies
    }

    #[test]
    fn store_get_returns_correct_program() {
        let store = ProgramStore::new();
        let p = programs::fibonacci(10);
        let id = store.add(p.clone());
        let retrieved = store.get(&id).unwrap();
        assert_eq!(retrieved.id(), id);
    }

    // ── Assembler ─────────────────────────────────────────────────────────────

    #[test]
    fn assembler_fibonacci() {
        let src = r#"
; Fibonacci
load r0, 9
load r1, 0
load r2, 1
load r4, 1
loop:
  jz r0, end
  add r3, r1, r2
  mov r1, r2
  mov r2, r3
  sub r0, r0, r4
  jmp loop
end:
  halt
"#;
        let program = Assembler::new()
            .with_name("fibonacci")
            .assemble(src)
            .expect("asm failed");
        let state = run_to_completion(&program);
        assert_eq!(state.regs[2], 55, "fibonacci(10) should be 55");
    }

    #[test]
    fn assembler_unknown_instruction() {
        let result = Assembler::new().assemble("foo r0, 1");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("unknown instruction"));
    }

    #[test]
    fn assembler_undefined_label() {
        let result = Assembler::new().assemble("jmp nowhere");
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("undefined label"));
    }

    #[test]
    fn assembler_invalid_register() {
        let result = Assembler::new().assemble("load x0, 42");
        assert!(result.is_err());
    }

    #[test]
    fn assembler_comments_and_blank_lines() {
        let src = r#"
; this is a comment
  ; indented comment

    load r0, 99   ; inline comment
    halt
"#;
        let p = Assembler::new().assemble(src).unwrap();
        let state = run_to_completion(&p);
        assert_eq!(state.regs[0], 99);
    }

    #[test]
    fn assembler_hex_immediate() {
        let src = "load r0, 0xFF\nhalt";
        let p = Assembler::new().assemble(src).unwrap();
        let mut state = VMState::new(&p);
        state.run(&p).unwrap();
        assert_eq!(state.regs[0], 255);
    }

    #[test]
    fn assembled_and_inline_fibonacci_produce_same_id() {
        // Both paths should produce identical instruction sequences.
        let src = r#"
load r0, 4
load r1, 0
load r2, 1
load r4, 1
loop:
  jz r0, end
  add r3, r1, r2
  mov r1, r2
  mov r2, r3
  sub r0, r0, r4
  jmp loop
end:
  halt
"#;
        let asm_prog = Assembler::new().assemble(src).unwrap();
        let inline_prog = programs::fibonacci(5);

        // Same instructions → same ID
        assert_eq!(
            asm_prog.id(),
            inline_prog.id(),
            "assembler output should match inline program byte-for-byte"
        );
    }

    #[test]
    fn fibonacci_asm_matches_inline() {
        // File-based assembly should match the canonical inline program.
        let src = std::fs::read_to_string("programs/fibonacci.asm").unwrap();
        let asm_prog = Assembler::new().assemble(&src).unwrap();
        let inline_prog = programs::fibonacci(10);
        assert_eq!(asm_prog.id(), inline_prog.id());
    }
}
