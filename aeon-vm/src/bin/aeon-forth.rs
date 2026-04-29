use aeon_vm::{ForthPrototype, Inst, Program, VMState};
use std::env;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: aeon-forth <source.fs>");
        std::process::exit(2);
    };

    let source = std::fs::read_to_string(Path::new(&path))?;
    let program = Program::new(vec![Inst::Halt]);
    let mut state = VMState::new(&program);
    ForthPrototype::start(&mut state, &source)?;

    for value in ForthPrototype::run(&mut state)? {
        println!("{}", value);
    }

    Ok(())
}
