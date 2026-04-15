// bin/aeon-asm.rs — assembler CLI
// Usage: aeon-asm <input.asm> [-o output.aeon]

use aeon_vm::asm::Assembler;
use std::env;
use std::path::Path;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: aeon-asm <input.asm> [-o output.aeon]");
        std::process::exit(1);
    }

    let input_path = Path::new(&args[1]);
    let output_path: std::path::PathBuf = args.windows(2)
        .find(|w| w[0] == "-o")
        .map(|w| std::path::PathBuf::from(&w[1]))
        .unwrap_or_else(|| {
            // Default to writing the assembled file into the current working directory
            // rather than the input file's directory.
            let stem = input_path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unnamed");
            std::env::current_dir().unwrap().join(format!("{}.aeon", stem))
        });

    let source = std::fs::read_to_string(input_path).unwrap_or_else(|e| {
        eprintln!("Cannot read {}: {}", input_path.display(), e);
        std::process::exit(1);
    });

    let name = input_path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unnamed");

    let program = Assembler::new().with_name(name).assemble(&source).unwrap_or_else(|e| {
        eprintln!("Assembly error: {}", e);
        std::process::exit(1);
    });

    program.save(&output_path).unwrap_or_else(|e| {
        eprintln!("Cannot write {}: {}", output_path.display(), e);
        std::process::exit(1);
    });

    let id = program.id();
    println!("Assembled: {} → {}", input_path.display(), output_path.display());
    println!("  Instructions: {}", program.instruction_count());
    println!("  Program ID:   {:x}{:x}{:x}{:x}...", id[0], id[1], id[2], id[3]);
}
