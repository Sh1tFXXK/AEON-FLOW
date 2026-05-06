use aeon_store::{
    hex_cid, parse_cid_hex, Account, Blob, CIDStore, Context, Message, SyncEngine, Thread, CID,
};
use aeon_vm::daemon::{default_socket_path, default_state_dir, send_request, serve, DaemonRequest};
use aeon_vm::snapshot::Snapshot;
use std::env;
use std::io::{self, Write};
use std::path::Path;

fn main() {
    if let Err(err) = run() {
        eprintln!("[aeon] {}", err);
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("log") => {
            let target = args
                .get(2)
                .ok_or_else(|| "usage: aeon log <snapshot.snap|vm-id>".to_string())?;
            if Path::new(target).exists() {
                print_snapshot_log(Path::new(target))
            } else {
                print_daemon_response("log", &[target.clone()])
            }
        }
        Some("daemon") => serve(&default_socket_path(), default_state_dir()),
        Some("ps") => print_daemon_response("ps", &[]),
        Some("run") => {
            let path = args
                .get(2)
                .ok_or_else(|| "usage: aeon run <file.aeon>".to_string())?;
            print_daemon_response("run", &[path.clone()])
        }
        Some("pause") | Some("resume") | Some("share") => {
            let cmd = args[1].as_str();
            let id = args
                .get(2)
                .ok_or_else(|| format!("usage: aeon {} <id>", cmd))?;
            print_daemon_response(cmd, &[id.clone()])
        }
        Some("migrate") => {
            let id = args
                .get(2)
                .ok_or_else(|| "usage: aeon migrate <id> --to <host:port>".to_string())?;
            let to = args
                .windows(2)
                .find(|window| window[0] == "--to")
                .map(|window| window[1].clone())
                .ok_or_else(|| "usage: aeon migrate <id> --to <host:port>".to_string())?;
            print_daemon_response("migrate", &[id.clone(), to])
        }
        Some("devices") => print_daemon_response("devices", &[]),
        Some("put") => {
            let path = args
                .get(2)
                .ok_or_else(|| "usage: aeon put <file>".to_string())?;
            data_put(path)
        }
        Some("get") => {
            let cid = args
                .get(2)
                .ok_or_else(|| "usage: aeon get <CID>".to_string())?;
            data_get(cid)
        }
        Some("ls") => data_ls(
            args.windows(2)
                .find(|w| w[0] == "--kind")
                .map(|w| w[1].as_str()),
        ),
        Some("account") => {
            let display_name = args
                .get(2)
                .ok_or_else(|| "usage: aeon account <display-name> <public-key-hex>".to_string())?;
            let public_key = args
                .get(3)
                .ok_or_else(|| "usage: aeon account <display-name> <public-key-hex>".to_string())?;
            account_create(display_name, public_key)
        }
        Some("whoami") => {
            println!(
                "{}",
                env::var("AEON_ACCOUNT_ID").unwrap_or_else(|_| "unconfigured".to_string())
            );
            Ok(())
        }
        Some("accounts") => {
            println!(
                "local account registry is not implemented; use AEON_ACCOUNT_ID or `aeon account`"
            );
            Ok(())
        }
        Some("ctx") => context_cmd(&args[2..]),
        Some("chat") => chat_cmd(&args[2..]),
        Some("sync") => sync_cmd(&args[2..]),
        _ => Err(usage()),
    }
}

fn print_snapshot_log(path: &Path) -> Result<(), String> {
    let snap = Snapshot::load(path).map_err(|err| format!("load {}: {}", path.display(), err))?;
    snap.event_log.verify()?;
    for line in snap.event_log.lines() {
        println!("{}", line);
    }
    Ok(())
}

fn print_daemon_response(cmd: &str, args: &[String]) -> Result<(), String> {
    let response = send_request(
        &default_socket_path(),
        &DaemonRequest {
            cmd: cmd.to_string(),
            args: args.to_vec(),
        },
    )?;
    print!("{}", response);
    Ok(())
}

fn data_put(path: &str) -> Result<(), String> {
    let mut store = open_store()?;
    let blob = Blob::from_file(Path::new(path)).map_err(|err| err.to_string())?;
    let cid = store.put(blob.clone()).map_err(|err| err.to_string())?;
    println!("CID: {}", hex_cid(&cid));
    println!("Stored {} bytes", blob.data.len());
    println!("MIME: {}", blob.mime);
    Ok(())
}

fn data_get(cid: &str) -> Result<(), String> {
    let cid = parse_cli_cid(cid)?;
    let mut store = open_store()?;
    let blob = store
        .get(&cid)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "CID not found".to_string())?;
    io::stdout()
        .write_all(&blob.data)
        .map_err(|err| err.to_string())
}

fn data_ls(kind_filter: Option<&str>) -> Result<(), String> {
    let mut store = open_store()?;
    let cids = store.list().map_err(|err| err.to_string())?;
    for cid in &cids {
        let Some(blob) = store.get(cid).map_err(|err| err.to_string())? else {
            continue;
        };
        if kind_filter.is_some_and(|kind| !blob.mime.contains(kind)) {
            continue;
        }
        println!("{} {} {} bytes", hex_cid(cid), blob.mime, blob.data.len());
    }
    println!(
        "{} blobs, {} bytes total",
        cids.len(),
        store.total_size_bytes().map_err(|err| err.to_string())?
    );
    Ok(())
}

fn account_create(display_name: &str, public_key: &str) -> Result<(), String> {
    let account = Account::from_public_key(display_name, parse_cli_cid(public_key)?);
    println!("account: {}", hex_cid(&account.id));
    println!("display_name: {}", account.display_name);
    Ok(())
}

fn context_cmd(args: &[String]) -> Result<(), String> {
    let mut store = open_store()?;
    match args {
        [mode, name, owner_flag, owner] if mode == "new" && owner_flag == "--owner" => {
            let context = Context::new(name, parse_cli_cid(owner)?);
            print_context(&mut store, &context)
        }
        [mode, context_cid, node_cid, by_flag, by] if mode == "add" && by_flag == "--by" => {
            let mut context = load_context(&mut store, parse_cli_cid(context_cid)?)?;
            context
                .add_node(parse_cli_cid(node_cid)?, parse_cli_cid(by)?, now_millis())
                .map_err(|err| err.to_string())?;
            print_context(&mut store, &context)
        }
        [mode, context_cid] if mode == "history" => {
            let context = load_context(&mut store, parse_cli_cid(context_cid)?)?;
            println!("context: {} ({})", context.name, context.id);
            for event in context.events {
                println!("{event:?}");
            }
            Ok(())
        }
        _ => Err("usage: aeon ctx new <name> --owner <account-id> | ctx add <ctx-cid> <node-cid> --by <account-id> | ctx history <ctx-cid>".to_string()),
    }
}

fn chat_cmd(args: &[String]) -> Result<(), String> {
    let mut store = open_store()?;
    match args {
        [mode, thread_id] if mode == "new" => {
            let thread = Thread::new(thread_id, Vec::new(), None);
            print_thread(&mut store, &thread)
        }
        [mode, thread_cid, text, author_flag, author] if mode == "send" && author_flag == "--author" => {
            let mut thread = load_thread(&mut store, parse_cli_cid(thread_cid)?)?;
            let content_cid = store
                .put(Blob::from_text(text))
                .map_err(|err| err.to_string())?;
            let message = Message::new(&thread.id, author, content_cid, None, now_millis());
            let message_cid = store
                .put(message.to_blob().map_err(|err| err.to_string())?)
                .map_err(|err| err.to_string())?;
            thread.add_message(&message);
            print_thread(&mut store, &thread)?;
            println!("Message CID: {}", hex_cid(&message_cid));
            Ok(())
        }
        [mode, thread_cid] if mode == "ls" => {
            let thread = load_thread(&mut store, parse_cli_cid(thread_cid)?)?;
            for message_cid in thread.messages {
                let message = load_message(&mut store, message_cid)?;
                let content = store
                    .get(&message.content_cid)
                    .map_err(|err| err.to_string())?
                    .and_then(|blob| blob.as_text().map(ToOwned::to_owned))
                    .unwrap_or_else(|| "<binary>".to_string());
                println!("{}: {}", message.author, content);
            }
            Ok(())
        }
        _ => Err("usage: aeon chat new <thread-id> | chat send <thread-cid> <msg> --author <name> | chat ls <thread-cid>".to_string()),
    }
}

fn sync_cmd(args: &[String]) -> Result<(), String> {
    let store = open_store()?;
    let mut engine = SyncEngine::new(store, local_device_id());
    match args {
        [mode, port] if mode == "--listen" => {
            let addr = if port.contains(':') {
                port.clone()
            } else {
                format!("0.0.0.0:{port}")
            };
            println!("listening on {addr}");
            let report = engine.listen_once(&addr).map_err(|err| err.to_string())?;
            println!(
                "sync complete: requested {}, received {}, sent {}",
                report.requested, report.received, report.sent
            );
            Ok(())
        }
        [mode, cid, peer_flag, peer] if mode == "--announce" && peer_flag == "--peer" => {
            let cid = parse_cli_cid(cid)?;
            let report = engine
                .announce_to(cid, peer)
                .map_err(|err| err.to_string())?;
            println!(
                "announced {} to {}: sent {}, received {}",
                hex_cid(&cid),
                peer,
                report.sent,
                report.received
            );
            Ok(())
        }
        _ => Err(
            "usage: aeon sync --listen <port|addr> | sync --announce <cid> --peer <addr>"
                .to_string(),
        ),
    }
}

fn open_store() -> Result<CIDStore, String> {
    CIDStore::new(CIDStore::default_path()).map_err(|err| err.to_string())
}

fn parse_cli_cid(cid: &str) -> Result<CID, String> {
    parse_cid_hex(cid)
}

fn load_context(store: &mut CIDStore, cid: CID) -> Result<Context, String> {
    let blob = store
        .get(&cid)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "context CID not found".to_string())?;
    Context::from_blob(&blob).map_err(|err| err.to_string())
}

fn print_context(store: &mut CIDStore, context: &Context) -> Result<(), String> {
    let cid = store
        .put(context.to_blob().map_err(|err| err.to_string())?)
        .map_err(|err| err.to_string())?;
    println!("Context ID: {}", context.id);
    println!("CID: {}", hex_cid(&cid));
    println!("events: {}", context.events.len());
    Ok(())
}

fn load_thread(store: &mut CIDStore, cid: CID) -> Result<Thread, String> {
    let blob = store
        .get(&cid)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "thread CID not found".to_string())?;
    Thread::from_blob(&blob).map_err(|err| err.to_string())
}

fn load_message(store: &mut CIDStore, cid: CID) -> Result<Message, String> {
    let blob = store
        .get(&cid)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "message CID not found".to_string())?;
    Message::from_blob(&blob).map_err(|err| err.to_string())
}

fn print_thread(store: &mut CIDStore, thread: &Thread) -> Result<(), String> {
    let cid = store
        .put(thread.to_blob().map_err(|err| err.to_string())?)
        .map_err(|err| err.to_string())?;
    println!("Thread ID: {}", thread.id);
    println!("CID: {}", hex_cid(&cid));
    println!("messages: {}", thread.messages.len());
    Ok(())
}

fn local_device_id() -> [u8; 16] {
    let source = env::var("AEON_DEVICE_ID")
        .or_else(|_| env::var("HOSTNAME"))
        .unwrap_or_else(|_| "aeon-local-device".to_string());
    let hash = blake3::hash(source.as_bytes());
    let mut id = [0u8; 16];
    id.copy_from_slice(&hash.as_bytes()[..16]);
    id
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn usage() -> String {
    "usage: aeon daemon|ps|run|pause|resume|migrate|log|devices|share|put|get|ls|account|whoami|accounts|ctx|chat|sync".to_string()
}
