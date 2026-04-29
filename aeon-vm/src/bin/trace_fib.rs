fn main() {
    let n: u64 = std::env::var("AEON_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    let p = aeon_vm::program::programs::fibonacci(n);
    let mut s = aeon_vm::vm::VMState::new(&p);
    println!("Starting trace of fibonacci({})", n);
    loop {
        let inst = p
            .instructions
            .get(s.pc)
            .map(|i| i.disassemble())
            .unwrap_or("<none>".into());
        let res = s.step(&p);
        println!(
            "step {} pc={} inst={}\n regs[0..5]={:?} steps={} result={:?}",
            s.steps,
            s.pc,
            inst,
            &s.regs[0..5],
            s.steps,
            res
        );
        if let aeon_vm::vm::StepResult::Halted = res {
            break;
        }
    }
}
