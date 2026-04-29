use aeon_vm::daemon::{default_socket_path, default_state_dir, serve};

fn main() {
    if let Err(err) = serve(&default_socket_path(), default_state_dir()) {
        eprintln!("[aeon-daemon] {}", err);
        std::process::exit(1);
    }
}
