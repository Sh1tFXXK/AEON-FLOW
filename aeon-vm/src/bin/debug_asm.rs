fn main() {
    let src = r#"
; Fibonacci
load r0, 10
load r1, 0
load r2, 1
load r4, 1
loop:
  add r3, r1, r2
  mov r1, r2
  mov r2, r3
  sub r0, r0, r4
  jz r0, end
  jmp loop
end:
  halt
"#;
    let asm = aeon_vm::asm::Assembler::new().with_name("debug-fib");
    let prog = asm.assemble(src).expect("asm failed");
    println!("Assembled disassembly:\n{}", prog.disassemble());
    println!("Program id: {:?}", prog.id());
    let prog2 = aeon_vm::program::programs::fibonacci(10);
    println!("Inline disassembly:\n{}", prog2.disassemble());
    println!("Inline id: {:?}", prog2.id());

    let mut s1 = aeon_vm::vm::VMState::new(&prog);
    let steps = s1.run(&prog).expect("run failed");
    println!("Assembled run steps={}, r2={}", steps, s1.regs[2]);

    let mut s2 = aeon_vm::vm::VMState::new(&prog2);
    let steps2 = s2.run(&prog2).expect("run failed");
    println!("Inline run steps={}, r2={}", steps2, s2.regs[2]);
}
