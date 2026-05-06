#[tokio::main]
async fn main() {
    if let Err(err) = aeon_vm::daemon::serve(&aeon_vm::daemon::default_socket_path(), aeon_vm::daemon::default_state_dir()).await {
        eprintln!("[aeon-daemon] {}", err);
        std::process::exit(1);
    }
}
