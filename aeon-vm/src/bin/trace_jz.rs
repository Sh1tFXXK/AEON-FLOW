fn main() {
    use aeon_vm::inst::Inst;
    use aeon_vm::vm::{StepResult, VMState};
    let p = aeon_vm::program::Program::new(vec![
        Inst::LoadImm { dst: 0, val: 0 },
        Inst::Jz { cond: 0, off: 2 },
        Inst::LoadImm { dst: 1, val: 99 },
        Inst::LoadImm { dst: 1, val: 42 },
        Inst::Halt,
    ]);
    let mut s = VMState::new(&p);
    println!("Tracing jz program");
    for i in 0..10 {
        if s.pc >= p.instructions.len() {
            println!("pc OOB {}", s.pc);
            break;
        }
        println!(
            "step {} pc={} inst={}",
            i,
            s.pc,
            p.instructions[s.pc].disassemble()
        );
        let res = s.step(&p);
        println!(" regs[0..3]={:?} result={:?}", &s.regs[0..3], res);
        if let StepResult::Halted = res {
            println!("halted");
            break;
        }
    }
}
