fn main() {
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
    let asm_prog = aeon_vm::asm::Assembler::new().assemble(src).unwrap();
    let inline_prog = aeon_vm::program::programs::fibonacci(5);
    println!("ASM disasm:\n{}", asm_prog.disassemble());
    println!("INLINE disasm:\n{}", inline_prog.disassemble());
    println!("ASM id: {:?}", asm_prog.id());
    println!("INLINE id: {:?}", inline_prog.id());
}
