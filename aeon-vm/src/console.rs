use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crate::editor::{Inspector, SnapshotEditor};
use crate::session::{SessionId, SharedContext};
use crate::snapshot::Snapshot;
use crate::store::ProgramStore;

pub struct FlowConsole {
    session: SessionId,
    context_id: String,
    context: Arc<RwLock<SharedContext>>,
}

pub enum ConsoleResult {
    Resume(Snapshot),
    Discard(Snapshot),
    Quit,
}

impl FlowConsole {
    pub fn new(session: SessionId, base_snapshot: Snapshot) -> Self {
        let context_id = format!("ctx-{}", short_id(&base_snapshot.program_id));
        let context = Arc::new(RwLock::new(SharedContext::new(
            context_id.clone(),
            base_snapshot,
            session.clone(),
        )));
        FlowConsole {
            session,
            context_id,
            context,
        }
    }

    pub fn run(&self, _store: &ProgramStore) -> ConsoleResult {
        self.print_welcome();

        loop {
            print!("[{}] aeon> ", self.session);
            io::stdout().flush().ok();

            let mut line = String::new();
            match io::stdin().read_line(&mut line) {
                Ok(0) | Err(_) => return ConsoleResult::Quit,
                Ok(_) => {}
            }

            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.split_whitespace().collect();
            match parts[0] {
                "info" | "i" => Inspector::new(&self.current_snapshot()).summary(),
                "regs" | "r" => {
                    let snap = self.current_snapshot();
                    let start = parts.get(1).and_then(|v| parse_u8(v)).unwrap_or(0);
                    let end = parts.get(2).and_then(|v| parse_u8(v)).unwrap_or(15);
                    Inspector::new(&snap).dump_regs(start, end);
                }
                "heap" | "h" => {
                    let snap = self.current_snapshot();
                    let addr = parts.get(1).and_then(|v| parse_usize(v)).unwrap_or(0);
                    let len = parts.get(2).and_then(|v| parse_usize(v)).unwrap_or(64);
                    Inspector::new(&snap).dump_heap(addr, len);
                }
                "diff" | "d" => self.print_diff(),
                "history" | "log" => self.ctx_read().print_timeline(),
                "set" => self.cmd_set(&parts[1..]),
                "say" | "s" => {
                    let text = line.split_once(' ').map(|(_, text)| text).unwrap_or("");
                    self.ctx_write().post_message(self.session.clone(), text);
                }
                "share" => self.cmd_share(&parts[1..]),
                "join" => self.cmd_join(&parts[1..]),
                "sessions" => self.print_sessions(),
                "resume" => return ConsoleResult::Resume(self.current_snapshot()),
                "discard" => return ConsoleResult::Discard(self.base_snapshot()),
                "quit" | "q" | "exit" => return ConsoleResult::Quit,
                "help" | "?" => self.print_help(),
                other => println!("unknown command: {}", other),
            }
        }
    }

    fn cmd_set(&self, args: &[&str]) {
        let result = (|| -> Result<(), String> {
            let snap = self.current_snapshot();
            let patchset = match args {
                ["reg", reg, val] => {
                    let reg = parse_u8(reg).ok_or_else(|| "expected register index".to_string())?;
                    let val =
                        parse_u64(val).ok_or_else(|| "expected register value".to_string())?;
                    SnapshotEditor::new(&snap, format!("set r{} = {}", reg, val))
                        .set_reg(reg, val)?
                        .build()
                }
                ["pc", val] => {
                    let val = parse_usize(val).ok_or_else(|| "expected pc value".to_string())?;
                    SnapshotEditor::new(&snap, format!("set pc = {}", val))
                        .set_pc(val)?
                        .build()
                }
                _ => return Err("usage: set reg <n> <val> | set pc <val>".into()),
            };

            let description = patchset.description.clone();
            self.ctx_write()
                .apply_patch(self.session.clone(), description, patchset)?;
            Ok(())
        })();

        match result {
            Ok(()) => println!("applied"),
            Err(err) => println!("error: {}", err),
        }
    }

    fn print_diff(&self) {
        let ctx = self.ctx_read();
        let current = match ctx.current_snapshot() {
            Ok(snap) => snap,
            Err(err) => {
                println!("error: {}", err);
                return;
            }
        };
        for line in Inspector::diff(&ctx.base_snapshot, &current) {
            println!("{}", line);
        }
    }

    fn print_sessions(&self) {
        let ctx = self.ctx_read();
        for session in &ctx.connected_sessions {
            println!("{}", session);
        }
    }

    fn cmd_share(&self, args: &[&str]) {
        let Some(context_id) = args.first() else {
            println!("usage: share <context-id>");
            return;
        };
        let path = context_path(context_id);
        let bytes = self.ctx_read().to_bytes();
        match fs::write(&path, bytes) {
            Ok(()) => println!("shared context saved to {}", path.display()),
            Err(err) => println!("error saving context: {}", err),
        }
    }

    fn cmd_join(&self, args: &[&str]) {
        let Some(context_id) = args.first() else {
            println!("usage: join <context-id|file.ctx>");
            return;
        };
        let path = context_path(context_id);
        let data = match fs::read(&path) {
            Ok(data) => data,
            Err(err) => {
                println!("error reading {}: {}", path.display(), err);
                return;
            }
        };
        let foreign = match SharedContext::from_bytes(&data) {
            Ok(ctx) => ctx,
            Err(err) => {
                println!("error parsing context: {}", err);
                return;
            }
        };

        let mut current = self.ctx_write();
        if foreign.base_snapshot.program_id() != current.base_snapshot.program_id() {
            println!("error: context ProgramId does not match current snapshot");
            return;
        }

        let mut added_patches = 0;
        for patch in foreign.patches {
            let exists = current
                .patches
                .iter()
                .any(|item| item.clock == patch.clock && item.author == patch.author);
            if !exists {
                current.patches.push(patch);
                added_patches += 1;
            }
        }

        let mut added_messages = 0;
        for message in foreign.messages {
            let exists = current
                .messages
                .iter()
                .any(|item| item.clock == message.clock && item.author == message.author);
            if !exists {
                current.messages.push(message);
                added_messages += 1;
            }
        }

        for session in foreign.connected_sessions {
            current.join(session);
        }

        println!(
            "joined {} ({} patch(es), {} message(s))",
            path.display(),
            added_patches,
            added_messages
        );
    }

    fn ctx_read(&self) -> std::sync::RwLockReadGuard<'_, SharedContext> {
        self.context.read().unwrap()
    }

    fn ctx_write(&self) -> std::sync::RwLockWriteGuard<'_, SharedContext> {
        self.context.write().unwrap()
    }

    fn current_snapshot(&self) -> Snapshot {
        self.ctx_read().current_snapshot().unwrap_or_else(|err| {
            eprintln!("failed to build current snapshot: {}", err);
            std::process::exit(1);
        })
    }

    fn base_snapshot(&self) -> Snapshot {
        self.ctx_read().base_snapshot.clone()
    }

    fn print_welcome(&self) {
        let snap = self.current_snapshot();
        println!(
            "AEON Flow Console session={} context={} pc={} steps={}",
            self.session, self.context_id, snap.pc, snap.steps
        );
        println!("type 'help' for commands, 'resume' to continue");
    }

    fn print_help(&self) {
        println!("info | regs [start] [end] | heap [addr] [len] | diff | history | sessions");
        println!("set reg <n> <val> | set pc <val>");
        println!("say <message> | share <context-id> | join <context-id|file.ctx>");
        println!("resume | discard | quit");
    }
}

fn context_path(value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.extension().and_then(|ext| ext.to_str()) == Some("ctx") {
        path.to_path_buf()
    } else {
        PathBuf::from(format!("{}.ctx", value))
    }
}

fn parse_u8(value: &str) -> Option<u8> {
    parse_usize(value).and_then(|value| u8::try_from(value).ok())
}

fn parse_usize(value: &str) -> Option<usize> {
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        usize::from_str_radix(hex, 16).ok()
    } else {
        value.parse().ok()
    }
}

fn parse_u64(value: &str) -> Option<u64> {
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u64::from_str_radix(hex, 16).ok()
    } else {
        value.parse().ok()
    }
}

fn short_id(id: &[u8; 32]) -> String {
    format!("{:02x}{:02x}{:02x}{:02x}", id[0], id[1], id[2], id[3])
}
