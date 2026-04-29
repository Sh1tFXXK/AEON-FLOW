// src/asm.rs
// Text assembler: .asm source → Program.
//
// Syntax:
//   Comments:   ; this is a comment
//   Labels:     loop:
//   Registers:  r0 .. r255
//   Numbers:    decimal (42) or hexadecimal (0x2a)
//
// Instructions (case-insensitive):
//   load  r<dst>, <imm>       r[dst] = imm
//   mov   r<dst>, r<src>      r[dst] = r[src]
//   add   r<dst>, r<a>, r<b>  r[dst] = r[a] + r[b]
//   sub   r<dst>, r<a>, r<b>  r[dst] = r[a] - r[b]
//   mul   r<dst>, r<a>, r<b>  r[dst] = r[a] * r[b]
//   jz    r<cond>, <label>    if r[cond]==0: jump to label
//   jmp   <label>             unconditional jump
//   call  <label>             call subroutine
//   ret                       return from subroutine
//   alloc r<dst>, r<size>     allocate heap bytes; r[dst] = start address
//   loadmem r<dst>, r<addr>   r[dst] = heap[r[addr]]
//   storemem r<addr>, r<src>  heap[r[addr]] = r[src] as u8
//   syscall <num>             call VM service with register arguments
//   halt                      stop
//
// Example (fibonacci.asm):
//   load r0, 10
//   load r1, 0
//   load r2, 1
//   load r4, 1
//   loop:
//     add  r3, r1, r2
//     mov  r1, r2
//     mov  r2, r3
//     sub  r0, r0, r4
//     jz   r0, end
//     jmp  loop
//   end:
//     halt

use crate::inst::Inst;
use crate::program::Program;

#[derive(Debug)]
pub struct AsmError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for AsmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

// During first pass, jumps to labels are stored as placeholders.
#[derive(Debug, Clone)]
enum RawInst {
    Resolved(Inst),
    JzLabel { cond: u8, label: String },
    JmpLabel { label: String },
    CallLabel { label: String },
}

pub struct Assembler {
    name: String,
}

impl Assembler {
    pub fn new() -> Self {
        Assembler {
            name: "unnamed".into(),
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Assemble source text into a Program.
    pub fn assemble(&self, source: &str) -> Result<Program, AsmError> {
        let _source_hash = *blake3::hash(source.as_bytes()).as_bytes();

        // Pass 1: collect labels and raw instructions.
        let mut labels: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let mut raw: Vec<(usize, RawInst)> = Vec::new(); // (source_line, inst)

        for (line_idx, line) in source.lines().enumerate() {
            let line_no = line_idx + 1;
            let line = strip_comment(line).trim();
            if line.is_empty() {
                continue;
            }

            if line.ends_with(':') {
                let label = line.trim_end_matches(':').trim().to_lowercase();
                if label.is_empty() {
                    return Err(AsmError {
                        line: line_no,
                        message: "empty label".into(),
                    });
                }
                labels.insert(label, raw.len());
                continue;
            }

            let inst = self.parse_raw(line, line_no)?;
            raw.push((line_no, inst));
        }

        // Pass 2: resolve label references to relative offsets.
        let mut instructions: Vec<Inst> = Vec::with_capacity(raw.len());
        for (i, (line_no, raw_inst)) in raw.iter().enumerate() {
            let resolved = match raw_inst {
                RawInst::Resolved(inst) => inst.clone(),

                RawInst::JzLabel { cond, label } => {
                    let target = resolve_label(&labels, label, *line_no)?;
                    // offset is relative to the current instruction for VM semantics
                    let off = target as isize - i as isize;
                    Inst::Jz { cond: *cond, off }
                }

                RawInst::JmpLabel { label } => {
                    let target = resolve_label(&labels, label, *line_no)?;
                    // offset is relative to the current instruction for VM semantics
                    let off = target as isize - i as isize;
                    Inst::Jump { offset: off }
                }

                RawInst::CallLabel { label } => {
                    let target = resolve_label(&labels, label, *line_no)?;
                    Inst::Call {
                        addr: target as usize,
                    }
                }
            };
            instructions.push(resolved);
        }

        Ok(Program::from_parts(self.name.clone(), instructions))
    }

    fn parse_raw(&self, line: &str, line_no: usize) -> Result<RawInst, AsmError> {
        let err = |msg: &str| AsmError {
            line: line_no,
            message: msg.into(),
        };

        // Tokenize: split on whitespace and commas.
        let tokens: Vec<&str> = line
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|t| !t.is_empty())
            .collect();

        if tokens.is_empty() {
            return Err(err("empty instruction"));
        }

        let op = tokens[0].to_lowercase();

        let inst = match op.as_str() {
            "load" => {
                require_tokens(&tokens, 3, line_no)?;
                let dst = parse_reg(tokens[1], line_no)?;
                let val = parse_u64(tokens[2], line_no)?;
                RawInst::Resolved(Inst::LoadImm { dst, val })
            }
            "mov" => {
                require_tokens(&tokens, 3, line_no)?;
                let dst = parse_reg(tokens[1], line_no)?;
                let src = parse_reg(tokens[2], line_no)?;
                RawInst::Resolved(Inst::Mov { dst, src })
            }
            "add" => {
                require_tokens(&tokens, 4, line_no)?;
                let dst = parse_reg(tokens[1], line_no)?;
                let a = parse_reg(tokens[2], line_no)?;
                let b = parse_reg(tokens[3], line_no)?;
                RawInst::Resolved(Inst::Add { dst, a, b })
            }
            "sub" => {
                require_tokens(&tokens, 4, line_no)?;
                let dst = parse_reg(tokens[1], line_no)?;
                let a = parse_reg(tokens[2], line_no)?;
                let b = parse_reg(tokens[3], line_no)?;
                RawInst::Resolved(Inst::Sub { dst, a, b })
            }
            "mul" => {
                require_tokens(&tokens, 4, line_no)?;
                let dst = parse_reg(tokens[1], line_no)?;
                let a = parse_reg(tokens[2], line_no)?;
                let b = parse_reg(tokens[3], line_no)?;
                RawInst::Resolved(Inst::Mul { dst, a, b })
            }
            "jz" => {
                require_tokens(&tokens, 3, line_no)?;
                let cond = parse_reg(tokens[1], line_no)?;
                RawInst::JzLabel {
                    cond,
                    label: tokens[2].to_lowercase(),
                }
            }
            "jmp" => {
                require_tokens(&tokens, 2, line_no)?;
                RawInst::JmpLabel {
                    label: tokens[1].to_lowercase(),
                }
            }
            "call" => {
                require_tokens(&tokens, 2, line_no)?;
                RawInst::CallLabel {
                    label: tokens[1].to_lowercase(),
                }
            }
            "ret" => RawInst::Resolved(Inst::Ret),
            "alloc" => {
                require_tokens(&tokens, 3, line_no)?;
                let dst = parse_reg(tokens[1], line_no)?;
                let size = parse_reg(tokens[2], line_no)?;
                RawInst::Resolved(Inst::Alloc { dst, size })
            }
            "loadmem" => {
                require_tokens(&tokens, 3, line_no)?;
                let dst = parse_reg(tokens[1], line_no)?;
                let addr = parse_reg(tokens[2], line_no)?;
                RawInst::Resolved(Inst::LoadMem { dst, addr })
            }
            "storemem" => {
                require_tokens(&tokens, 3, line_no)?;
                let addr = parse_reg(tokens[1], line_no)?;
                let src = parse_reg(tokens[2], line_no)?;
                RawInst::Resolved(Inst::StoreMem { addr, src })
            }
            "print" => {
                require_tokens(&tokens, 2, line_no)?;
                let r = parse_reg(tokens[1], line_no)?;
                RawInst::Resolved(Inst::Print { r })
            }
            "syscall" => {
                require_tokens(&tokens, 2, line_no)?;
                let num = parse_u8(tokens[1], line_no)?;
                RawInst::Resolved(Inst::Syscall { num })
            }
            "halt" => RawInst::Resolved(Inst::Halt),

            _ => {
                return Err(AsmError {
                    line: line_no,
                    message: format!("unknown instruction: '{}'", tokens[0]),
                })
            }
        };

        Ok(inst)
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn strip_comment(line: &str) -> &str {
    match line.find(';') {
        Some(i) => &line[..i],
        None => line,
    }
}

fn require_tokens(tokens: &[&str], n: usize, line: usize) -> Result<(), AsmError> {
    if tokens.len() < n {
        Err(AsmError {
            line,
            message: format!(
                "'{}' requires {} tokens, got {}",
                tokens[0],
                n,
                tokens.len()
            ),
        })
    } else {
        Ok(())
    }
}

fn parse_reg(s: &str, line: usize) -> Result<u8, AsmError> {
    let s = s.to_lowercase();
    if !s.starts_with('r') {
        return Err(AsmError {
            line,
            message: format!("expected register like r0, got '{}'", s),
        });
    }
    s[1..].parse::<u8>().map_err(|_| AsmError {
        line,
        message: format!("invalid register number in '{}'", s),
    })
}

fn parse_u64(s: &str, line: usize) -> Result<u64, AsmError> {
    let parsed = if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16)
    } else {
        s.parse::<u64>()
    };

    parsed.map_err(|_| AsmError {
        line,
        message: format!("expected number, got '{}'", s),
    })
}

fn parse_u8(s: &str, line: usize) -> Result<u8, AsmError> {
    let value = parse_u64(s, line)?;
    u8::try_from(value).map_err(|_| AsmError {
        line,
        message: format!("expected u8, got '{}'", s),
    })
}

fn resolve_label(
    labels: &std::collections::HashMap<String, usize>,
    label: &str,
    line: usize,
) -> Result<usize, AsmError> {
    labels.get(label).copied().ok_or_else(|| AsmError {
        line,
        message: format!("undefined label '{}'", label),
    })
}
