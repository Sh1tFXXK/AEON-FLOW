use aeon_store::{hex_cid, parse_cid_hex, Account, Blob, CIDStore, Context, Identity, SyncEngine, CID};
use std::io::{self, Write};
use std::path::Path;

fn main() {
    if let Err(err) = run() {
        eprintln!("aeon-data: {err}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        usage();
        return Ok(());
    };

    let mut store = CIDStore::new(CIDStore::default_path())?;

    match command.as_str() {
        "put" => {
            let path = required_arg(args.next(), "put <file>")?;
            let blob = Blob::from_file(Path::new(&path))?;
            let cid = store.put(blob.clone())?;
            println!("CID: {}", hex_cid(&cid));
            println!("Stored {} bytes", blob.data.len());
            println!("MIME: {}", blob.mime);
        }
        "get" => {
            let cid = parse_cli_cid(&required_arg(args.next(), "get <cid>")?)?;
            let blob = store
                .get(&cid)?
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "CID not found"))?;
            io::stdout().write_all(&blob.data)?;
        }
        "has" => {
            let cid = parse_cli_cid(&required_arg(args.next(), "has <cid>")?)?;
            println!("{}", if store.has(&cid) { "yes" } else { "no" });
        }
        "list" => {
            let cids = store.list()?;
            println!(
                "{} blobs, {} bytes total",
                cids.len(),
                store.total_size_bytes()?
            );
            for cid in cids {
                println!("{}", hex_cid(&cid));
            }
        }
        "import" => {
            let path = required_arg(args.next(), "import <path>")?;
            let mut imported = ImportStats::default();
            import_path(Path::new(&path), &mut store, &mut imported)?;
            println!(
                "{} files imported, {} bytes stored",
                imported.files, imported.bytes
            );
        }
        "info" => {
            let cid = parse_cli_cid(&required_arg(args.next(), "info <cid>")?)?;
            let blob = store
                .get(&cid)?
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "CID not found"))?;
            println!("cid: {}", hex_cid(&cid));
            println!("mime: {}", blob.mime);
            println!("size: {} bytes", blob.data.len());
        }
        "identity" => handle_identity(args.collect())?,
        "account" => handle_account(args.collect())?,
        "context" => handle_context(args.collect(), &mut store)?,
        "sync" => handle_sync(args.collect(), store)?,
        _ => usage(),
    }

    Ok(())
}

#[derive(Default)]
struct ImportStats {
    files: u64,
    bytes: u64,
}

fn import_path(path: &Path, store: &mut CIDStore, stats: &mut ImportStats) -> io::Result<()> {
    if path.is_file() {
        let blob = Blob::from_file(path)?;
        stats.files += 1;
        stats.bytes += blob.data.len() as u64;
        store.put(blob)?;
        return Ok(());
    }

    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            import_path(&entry?.path(), store, stats)?;
        }
    }

    Ok(())
}

fn parse_cli_cid(hex: &str) -> io::Result<[u8; 32]> {
    parse_cid_hex(hex).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))
}


fn identity_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".aeon")
        .join("identity")
}

fn handle_identity(args: Vec<String>) -> io::Result<()> {
    match args.as_slice() {
        [mode] if mode == "new" => {
            let path = identity_path();
            if path.exists() {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, "identity already exists"));
            }
            let identity = Identity::load_or_create(&path)?;
            println!("✓ 新身份已创建: {}", identity.id_short());
            println!("私钥保存到 {}", path.display());
        }
        [mode] if mode == "show" => {
            let identity = Identity::load_or_create(&identity_path())?;
            println!("身份 ID: {}", identity.id_hex());
            println!("公钥: {}", identity.public_key_hex());
        }
        [mode] if mode == "export" => {
            let identity = Identity::load_or_create(&identity_path())?;
            println!("{}", identity.private_key_hex());
        }
        [mode, key] if mode == "import" => {
            let bytes = hex::decode(key)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
            if bytes.len() != 32 {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "identity key must be 32 bytes hex"));
            }
            let path = identity_path();
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, bytes)?;
            let identity = Identity::load_or_create(&path)?;
            println!("✓ 身份已导入: {}", identity.id_short());
            println!("私钥保存到 {}", path.display());
        }
        _ => {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "identity new|show|export|import <private-key-hex>"));
        }
    }
    Ok(())
}

fn handle_account(args: Vec<String>) -> io::Result<()> {
    match args.as_slice() {
        [display_name, public_key] => {
            let public_key = parse_cli_cid(public_key)?;
            let account = Account::from_public_key(display_name, public_key);
            println!("account: {}", hex_cid(&account.id));
            println!("display_name: {}", account.display_name);
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "account <display-name> <public-key-hex>",
            ));
        }
    }
    Ok(())
}

fn handle_context(args: Vec<String>, store: &mut CIDStore) -> io::Result<()> {
    match args.as_slice() {
        [mode, name, owner_flag, owner] if mode == "new" && owner_flag == "--owner" => {
            let owner = parse_cli_cid(owner)?;
            let context = Context::new(name, owner);
            print_stored_context(store, &context)?;
        }
        [mode, context_cid, node_cid, by_flag, by] if mode == "add" && by_flag == "--by" => {
            let mut context = load_context(store, parse_cli_cid(context_cid)?)?;
            context
                .add_node(parse_cli_cid(node_cid)?, parse_cli_cid(by)?, now_millis())
                .map_err(context_error)?;
            print_stored_context(store, &context)?;
        }
        [mode, context_cid, account_flag, account]
            if mode == "join" && account_flag == "--account" =>
        {
            let mut context = load_context(store, parse_cli_cid(context_cid)?)?;
            context.add_member(parse_cli_cid(account)?, now_millis());
            print_stored_context(store, &context)?;
        }
        [mode, context_cid, text, by_flag, by] if mode == "message" && by_flag == "--by" => {
            let mut context = load_context(store, parse_cli_cid(context_cid)?)?;
            context
                .message(text, parse_cli_cid(by)?, now_millis())
                .map_err(context_error)?;
            print_stored_context(store, &context)?;
        }
        [mode, context_cid] if mode == "history" => {
            let context = load_context(store, parse_cli_cid(context_cid)?)?;
            println!("context: {} ({})", context.name, context.id);
            for event in context.events {
                println!("{event:?}");
            }
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "context new/add/join/message/history",
            ));
        }
    }
    Ok(())
}

fn load_context(store: &mut CIDStore, cid: CID) -> io::Result<Context> {
    let blob = store
        .get(&cid)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "context CID not found"))?;
    Context::from_blob(&blob).map_err(json_error)
}

fn print_stored_context(store: &mut CIDStore, context: &Context) -> io::Result<()> {
    let blob = context.to_blob().map_err(json_error)?;
    let cid = store.put(blob)?;
    println!("Context ID: {}", context.id);
    println!("CID: {}", hex_cid(&cid));
    println!("events: {}", context.events.len());
    Ok(())
}

fn handle_sync(args: Vec<String>, store: CIDStore) -> io::Result<()> {
    let mut engine = SyncEngine::new(store, local_device_id());
    match args.as_slice() {
        [mode, port] if mode == "--listen" => {
            let addr = listen_addr(port);
            println!("listening on {addr}");
            let report = engine.listen_once(&addr)?;
            println!(
                "sync complete: requested {}, received {}, sent {}",
                report.requested, report.received, report.sent
            );
        }
        [mode, cid, peer_flag, peer] if mode == "--announce" && peer_flag == "--peer" => {
            let cid = parse_cli_cid(cid)?;
            let report = engine.announce_to(cid, peer)?;
            println!(
                "announced {} to {}: sent {}, received {}",
                hex_cid(&cid),
                peer,
                report.sent,
                report.received
            );
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "sync --listen <port|addr> OR sync --announce <cid> --peer <addr>",
            ));
        }
    }

    Ok(())
}

fn listen_addr(port_or_addr: &str) -> String {
    if port_or_addr.contains(':') {
        port_or_addr.to_string()
    } else {
        format!("0.0.0.0:{port_or_addr}")
    }
}

fn local_device_id() -> [u8; 16] {
    let source = std::env::var("AEON_DEVICE_ID")
        .or_else(|_| std::env::var("HOSTNAME"))
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

fn json_error(err: serde_json::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, err)
}

fn context_error(err: aeon_store::ContextError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, err)
}

fn required_arg(arg: Option<String>, usage_hint: &str) -> io::Result<String> {
    arg.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, usage_hint))
}

fn usage() {
    eprintln!("Usage:");
    eprintln!("  aeon-data put <file>");
    eprintln!("  aeon-data get <cid>");
    eprintln!("  aeon-data has <cid>");
    eprintln!("  aeon-data list");
    eprintln!("  aeon-data import <path>");
    eprintln!("  aeon-data info <cid>");
    eprintln!("  aeon-data identity new|show|export|import <private-key-hex>");
    eprintln!("  aeon-data account <display-name> <public-key-hex>");
    eprintln!("  aeon-data context new <name> --owner <account-id>");
    eprintln!("  aeon-data context add <context-cid> <node-cid> --by <account-id>");
    eprintln!("  aeon-data context join <context-cid> --account <account-id>");
    eprintln!("  aeon-data context message <context-cid> <text> --by <account-id>");
    eprintln!("  aeon-data context history <context-cid>");
    eprintln!("  aeon-data sync --listen <port|addr>");
    eprintln!("  aeon-data sync --announce <cid> --peer <addr>");
}
