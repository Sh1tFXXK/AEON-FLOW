use aeon_store::{hex_cid, parse_cid_hex, Blob, CIDStore};
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
}
