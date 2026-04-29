use crate::vfs::VirtualFS;
use crate::vm::VMState;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const STACK_REG: usize = 200;
pub const STACK_BASE: usize = 64 * 1024;
const STACK_LIMIT: usize = STACK_BASE + 64 * 1024;
const DICT_PATH: &str = "/forth/dict";
const RUNTIME_PATH: &str = "/forth/runtime";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForthError {
    StackUnderflow,
    StackOverflow,
    UnknownWord(String),
    UnknownVariable(String),
    BadDefinition(String),
    BadControl(String),
    Vfs(String),
    Codec(String),
    Utf8(String),
}

impl std::fmt::Display for ForthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ForthError::StackUnderflow => write!(f, "forth stack underflow"),
            ForthError::StackOverflow => write!(f, "forth stack overflow"),
            ForthError::UnknownWord(word) => write!(f, "unknown forth word: {}", word),
            ForthError::UnknownVariable(name) => write!(f, "unknown forth variable: {}", name),
            ForthError::BadDefinition(msg) => write!(f, "bad forth definition: {}", msg),
            ForthError::BadControl(msg) => write!(f, "bad forth control flow: {}", msg),
            ForthError::Vfs(msg) => write!(f, "forth vfs error: {}", msg),
            ForthError::Codec(msg) => write!(f, "forth codec error: {}", msg),
            ForthError::Utf8(msg) => write!(f, "forth utf-8 error: {}", msg),
        }
    }
}

impl std::error::Error for ForthError {}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Dictionary {
    words: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Runtime {
    tokens: Vec<String>,
    ip: usize,
    frames: Vec<Frame>,
    loops: Vec<LoopFrame>,
    envs: Vec<HashMap<String, u64>>,
    output: Vec<u64>,
    halted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Frame {
    tokens: Vec<String>,
    ip: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LoopFrame {
    start_ip: usize,
    index: u64,
    limit: u64,
}

pub struct ForthPrototype;

impl ForthPrototype {
    pub fn start(vm: &mut VMState, source: &str) -> Result<(), ForthError> {
        let (dict, tokens) = compile_source(&mut vm.vfs, source)?;
        vm.regs[STACK_REG] = STACK_BASE as u64;
        save(&mut vm.vfs, DICT_PATH, &dict)?;
        save(
            &mut vm.vfs,
            RUNTIME_PATH,
            &Runtime {
                tokens,
                ip: 0,
                frames: Vec::new(),
                loops: Vec::new(),
                envs: vec![HashMap::new()],
                output: Vec::new(),
                halted: false,
            },
        )
    }

    pub fn start_file(vm: &mut VMState, path: &str) -> Result<(), ForthError> {
        let bytes = read_raw(&mut vm.vfs, path)?;
        let source = String::from_utf8(bytes).map_err(|err| ForthError::Utf8(err.to_string()))?;
        Self::start(vm, &source)
    }

    pub fn run(vm: &mut VMState) -> Result<Vec<u64>, ForthError> {
        for _ in 0..100_000 {
            if Self::run_steps(vm, 1)? {
                return Self::output(vm);
            }
        }
        Err(ForthError::BadControl("step budget exhausted".into()))
    }

    pub fn run_steps(vm: &mut VMState, max_steps: usize) -> Result<bool, ForthError> {
        for _ in 0..max_steps {
            let dict: Dictionary = load(&mut vm.vfs, DICT_PATH)?;
            let mut runtime: Runtime = load(&mut vm.vfs, RUNTIME_PATH)?;
            if runtime.halted {
                return Ok(true);
            }
            step_once(vm, &dict, &mut runtime)?;
            let halted = runtime.halted;
            save(&mut vm.vfs, RUNTIME_PATH, &runtime)?;
            if halted {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn output(vm: &mut VMState) -> Result<Vec<u64>, ForthError> {
        let runtime: Runtime = load(&mut vm.vfs, RUNTIME_PATH)?;
        Ok(runtime.output)
    }

    pub fn stack(vm: &VMState) -> Result<Vec<u64>, ForthError> {
        let ptr = vm.regs[STACK_REG] as usize;
        if ptr < STACK_BASE || ptr > STACK_LIMIT || ptr > vm.heap.len() {
            return Err(ForthError::StackUnderflow);
        }
        if !(ptr - STACK_BASE).is_multiple_of(8) {
            return Err(ForthError::BadControl("stack pointer is unaligned".into()));
        }

        let mut values = Vec::new();
        let mut addr = STACK_BASE;
        while addr < ptr {
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&vm.heap[addr..addr + 8]);
            values.push(u64::from_le_bytes(bytes));
            addr += 8;
        }
        Ok(values)
    }

    pub fn dictionary_words(vm: &mut VMState) -> Result<Vec<String>, ForthError> {
        let dict: Dictionary = load(&mut vm.vfs, DICT_PATH)?;
        let mut words = dict.words.keys().cloned().collect::<Vec<_>>();
        words.sort();
        Ok(words)
    }
}

fn compile_source(
    vfs: &mut VirtualFS,
    source: &str,
) -> Result<(Dictionary, Vec<String>), ForthError> {
    let mut dict = Dictionary::default();
    let mut main = Vec::new();
    compile_into(vfs, source, &mut dict, &mut main, 0)?;
    Ok((dict, main))
}

fn compile_into(
    vfs: &mut VirtualFS,
    source: &str,
    dict: &mut Dictionary,
    main: &mut Vec<String>,
    depth: usize,
) -> Result<(), ForthError> {
    if depth > 16 {
        return Err(ForthError::BadDefinition("include depth exceeded".into()));
    }

    let tokens = tokenize(source);
    let mut i = 0;

    while i < tokens.len() {
        if tokens[i] == "include" {
            let path = tokens
                .get(i + 1)
                .ok_or_else(|| ForthError::BadDefinition("include missing path".into()))?;
            let bytes = read_raw(vfs, path)?;
            let source =
                String::from_utf8(bytes).map_err(|err| ForthError::Utf8(err.to_string()))?;
            compile_into(vfs, &source, dict, main, depth + 1)?;
            i += 2;
            continue;
        }

        if tokens[i] != ":" {
            if tokens[i] == ";" {
                return Err(ForthError::BadDefinition("unexpected ;".into()));
            }
            main.push(tokens[i].clone());
            i += 1;
            continue;
        }

        let name = tokens
            .get(i + 1)
            .ok_or_else(|| ForthError::BadDefinition("missing word name".into()))?
            .clone();
        i += 2;
        let start = i;
        while i < tokens.len() && tokens[i] != ";" {
            i += 1;
        }
        if i == tokens.len() {
            return Err(ForthError::BadDefinition(format!("{} missing ;", name)));
        }
        dict.words.insert(name, tokens[start..i].to_vec());
        i += 1;
    }

    Ok(())
}

fn tokenize(source: &str) -> Vec<String> {
    let mut out = String::with_capacity(source.len());
    let mut paren_comment = false;
    let mut line_comment = false;

    for ch in source.chars() {
        if line_comment {
            if ch == '\n' {
                line_comment = false;
                out.push(' ');
            }
            continue;
        }
        if paren_comment {
            if ch == ')' {
                paren_comment = false;
            }
            continue;
        }
        match ch {
            '\\' => line_comment = true,
            '(' => {
                paren_comment = true;
                out.push(' ');
            }
            _ => out.push(ch),
        }
    }

    out.split_whitespace()
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

fn step_once(vm: &mut VMState, dict: &Dictionary, runtime: &mut Runtime) -> Result<(), ForthError> {
    if runtime.ip >= runtime.tokens.len() {
        if let Some(frame) = runtime.frames.pop() {
            if runtime.envs.len() > 1 {
                runtime.envs.pop();
            }
            runtime.tokens = frame.tokens;
            runtime.ip = frame.ip;
        } else {
            runtime.halted = true;
        }
        return Ok(());
    }

    let token = runtime.tokens[runtime.ip].clone();
    runtime.ip += 1;
    match token.as_str() {
        "+" => binop(vm, u64::wrapping_add),
        "-" => binop(vm, u64::wrapping_sub),
        "*" => binop(vm, u64::wrapping_mul),
        "=" => compare(vm, |a, b| a == b),
        "<" => compare(vm, |a, b| a < b),
        ">" => compare(vm, |a, b| a > b),
        "dup" => {
            let value = pop(vm)?;
            push(vm, value)?;
            push(vm, value)
        }
        "swap" => {
            let b = pop(vm)?;
            let a = pop(vm)?;
            push(vm, b)?;
            push(vm, a)
        }
        "over" => {
            let b = pop(vm)?;
            let a = pop(vm)?;
            push(vm, a)?;
            push(vm, b)?;
            push(vm, a)
        }
        "drop" => pop(vm).map(|_| ()),
        "depth" => push(vm, stack_depth(vm)? as u64),
        "." => {
            runtime.output.push(pop(vm)?);
            Ok(())
        }
        "var" => {
            let name = next_token(runtime, "var")?;
            current_env(runtime).entry(name).or_insert(0);
            Ok(())
        }
        "set" => {
            let name = next_token(runtime, "set")?;
            let value = pop(vm)?;
            set_var(runtime, name, value);
            Ok(())
        }
        "get" => {
            let name = next_token(runtime, "get")?;
            let value = get_var(runtime, &name)?;
            push(vm, value)
        }
        "if" => {
            if pop(vm)? == 0 {
                skip_to(runtime, "if", "then")?;
            }
            Ok(())
        }
        "then" => Ok(()),
        "do" => {
            let start = pop(vm)?;
            let limit = pop(vm)?;
            if start >= limit {
                skip_to(runtime, "do", "loop")
            } else {
                runtime.loops.push(LoopFrame {
                    start_ip: runtime.ip,
                    index: start,
                    limit,
                });
                Ok(())
            }
        }
        "i" => {
            let index = runtime
                .loops
                .last()
                .ok_or_else(|| ForthError::BadControl("i outside loop".into()))?
                .index;
            push(vm, index)
        }
        "loop" => {
            let loop_frame = runtime
                .loops
                .last_mut()
                .ok_or_else(|| ForthError::BadControl("loop without do".into()))?;
            loop_frame.index = loop_frame.index.wrapping_add(1);
            if loop_frame.index < loop_frame.limit {
                runtime.ip = loop_frame.start_ip;
            } else {
                runtime.loops.pop();
            }
            Ok(())
        }
        word => {
            if let Some(body) = dict.words.get(word) {
                runtime.frames.push(Frame {
                    tokens: runtime.tokens.clone(),
                    ip: runtime.ip,
                });
                runtime.tokens = body.clone();
                runtime.ip = 0;
                runtime.envs.push(HashMap::new());
                Ok(())
            } else if let Ok(value) = word.parse::<u64>() {
                push(vm, value)
            } else {
                Err(ForthError::UnknownWord(word.into()))
            }
        }
    }
}

fn next_token(runtime: &mut Runtime, op: &str) -> Result<String, ForthError> {
    let token = runtime
        .tokens
        .get(runtime.ip)
        .ok_or_else(|| ForthError::BadControl(format!("{} missing operand", op)))?
        .clone();
    runtime.ip += 1;
    Ok(token)
}

fn skip_to(runtime: &mut Runtime, open: &str, close: &str) -> Result<(), ForthError> {
    let mut depth = 1;
    while runtime.ip < runtime.tokens.len() {
        let token = runtime.tokens[runtime.ip].as_str();
        runtime.ip += 1;
        if token == open {
            depth += 1;
        } else if token == close {
            depth -= 1;
            if depth == 0 {
                return Ok(());
            }
        }
    }
    Err(ForthError::BadControl(format!(
        "{} without {}",
        open, close
    )))
}

fn binop(vm: &mut VMState, op: fn(u64, u64) -> u64) -> Result<(), ForthError> {
    let b = pop(vm)?;
    let a = pop(vm)?;
    push(vm, op(a, b))
}

fn compare(vm: &mut VMState, op: fn(u64, u64) -> bool) -> Result<(), ForthError> {
    let b = pop(vm)?;
    let a = pop(vm)?;
    push(vm, u64::from(op(a, b)))
}

fn current_env(runtime: &mut Runtime) -> &mut HashMap<String, u64> {
    if runtime.envs.is_empty() {
        runtime.envs.push(HashMap::new());
    }
    runtime.envs.last_mut().expect("envs is non-empty")
}

fn set_var(runtime: &mut Runtime, name: String, value: u64) {
    if let Some(env) = runtime
        .envs
        .iter_mut()
        .rev()
        .find(|env| env.contains_key(&name))
    {
        env.insert(name, value);
    } else {
        current_env(runtime).insert(name, value);
    }
}

fn get_var(runtime: &Runtime, name: &str) -> Result<u64, ForthError> {
    runtime
        .envs
        .iter()
        .rev()
        .find_map(|env| env.get(name).copied())
        .ok_or_else(|| ForthError::UnknownVariable(name.into()))
}

fn stack_depth(vm: &VMState) -> Result<usize, ForthError> {
    let ptr = vm.regs[STACK_REG] as usize;
    if ptr < STACK_BASE || ptr > STACK_LIMIT || ptr > vm.heap.len() {
        return Err(ForthError::StackUnderflow);
    }
    Ok((ptr - STACK_BASE) / 8)
}

fn push(vm: &mut VMState, value: u64) -> Result<(), ForthError> {
    let ptr = vm.regs[STACK_REG] as usize;
    let end = ptr.checked_add(8).ok_or(ForthError::StackOverflow)?;
    if ptr < STACK_BASE || end > STACK_LIMIT || end > vm.heap.len() {
        return Err(ForthError::StackOverflow);
    }
    vm.heap[ptr..end].copy_from_slice(&value.to_le_bytes());
    vm.regs[STACK_REG] = end as u64;
    Ok(())
}

fn pop(vm: &mut VMState) -> Result<u64, ForthError> {
    let ptr = vm.regs[STACK_REG] as usize;
    if ptr <= STACK_BASE || ptr > STACK_LIMIT || ptr > vm.heap.len() {
        return Err(ForthError::StackUnderflow);
    }
    let start = ptr - 8;
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&vm.heap[start..ptr]);
    vm.regs[STACK_REG] = start as u64;
    Ok(u64::from_le_bytes(bytes))
}

fn save<T: Serialize>(vfs: &mut VirtualFS, path: &str, value: &T) -> Result<(), ForthError> {
    let payload = bincode::serialize(value).map_err(|err| ForthError::Codec(err.to_string()))?;
    let mut bytes = (payload.len() as u64).to_le_bytes().to_vec();
    bytes.extend_from_slice(&payload);
    let fd = vfs
        .open(path, true)
        .map_err(|err| ForthError::Vfs(err.to_string()))?;
    vfs.write(fd, &bytes)
        .map_err(|err| ForthError::Vfs(err.to_string()))?;
    vfs.close(fd)
        .map_err(|err| ForthError::Vfs(err.to_string()))
}

fn load<T: DeserializeOwned>(vfs: &mut VirtualFS, path: &str) -> Result<T, ForthError> {
    let bytes = read_raw(vfs, path)?;

    if bytes.len() < 8 {
        return Err(ForthError::Codec(format!("{} is missing length", path)));
    }
    let mut len_bytes = [0u8; 8];
    len_bytes.copy_from_slice(&bytes[..8]);
    let len = u64::from_le_bytes(len_bytes) as usize;
    let end = 8 + len;
    if bytes.len() < end {
        return Err(ForthError::Codec(format!("{} is truncated", path)));
    }
    bincode::deserialize(&bytes[8..end]).map_err(|err| ForthError::Codec(err.to_string()))
}

fn read_raw(vfs: &mut VirtualFS, path: &str) -> Result<Vec<u8>, ForthError> {
    let fd = vfs
        .open(path, false)
        .map_err(|err| ForthError::Vfs(err.to_string()))?;
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = vfs
            .read(fd, &mut chunk)
            .map_err(|err| ForthError::Vfs(err.to_string()))?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    vfs.close(fd)
        .map_err(|err| ForthError::Vfs(err.to_string()))?;
    Ok(bytes)
}
