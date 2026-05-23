# AEON VM

`aeon-vm` is the runnable-state prototype for AEON Flow. It provides a small VM
with snapshots, restore, migration, an editable heap, a daemon, and CLI tools.
It is not the desktop capture stack; start the full desktop from the repository
root with:

```powershell
.\scripts\aeon.ps1
```

## Build

```powershell
cd aeon-vm
cargo build --release
```

## Five-Minute Run

Assemble and run the sample program:

```powershell
cargo run --bin aeon-asm -- programs\fibonacci.asm -o fibonacci.aeon
cargo run --bin aeon-run -- fibonacci.aeon
```

Run to a bounded step count and save a snapshot:

```powershell
cargo run --bin aeon-run -- fibonacci.aeon --snap-at 5
```

Restore from a snapshot:

```powershell
cargo run --bin aeon-run -- fibonacci.aeon --restore fibonacci.snap
```

## Migration

Receiver:

```powershell
cargo run --bin aeon-recv -- fibonacci.aeon --port 9999
```

Sender:

```powershell
cargo run --bin aeon-send -- fibonacci.aeon --snap-at 5 --to 127.0.0.1:9999
```

## Console

```powershell
cargo run --bin aeon-console -- --load fibonacci.snap
```

Useful commands:

```text
history
regs
set reg 0 3
resume
```

## Collaboration Context Exchange

Console collaboration state is owned by `SharedContext`. Remote state is merged
through `SharedContext::merge_from`, which imports unique patches, messages, and
sessions while rejecting contexts from a different VM program.

The `collab_transport` module provides a tested one-shot TCP exchange for
embedding or future CLI wiring:

```rust
use aeon_vm::collab_transport::{exchange_context, serve_context_once};
```

This is a bounded context exchange, not a persistent live editing service.

## Daemon

The daemon manages multiple VM instances and exposes commands used by the
desktop process panel.

```powershell
cargo run --bin aeon-daemon
cargo run --bin aeon -- ps
```

On Unix, daemon commands use the configured Unix socket. On non-Unix targets,
the command transport uses localhost TCP by default.

## Account Registry

The unified `aeon` CLI stores local accounts in a JSON registry:

```powershell
cargo run --bin aeon -- account Alice <public-key-hex>
cargo run --bin aeon -- accounts
cargo run --bin aeon -- whoami
```

`AEON_ACCOUNT_REGISTRY` can override the registry file path for tests or
portable runs.

## CID Sync

The unified `aeon` CLI exposes the shared content-addressed sync service:

```powershell
cargo run --bin aeon -- sync --serve 7070
cargo run --bin aeon -- sync --announce <cid> --peer 127.0.0.1:7070
```

Use `--sessions <n>` with `--serve` for bounded smoke tests:

```powershell
cargo run --bin aeon -- sync --serve 127.0.0.1:7070 --sessions 2
```

## Known Limits

See [KNOWN_LIMITATIONS.md](KNOWN_LIMITATIONS.md). Important boundaries:

- VM event-set sync remains a deterministic reconciliation primitive, not a
  separate background event daemon.
- Console collaboration supports file merge and one-shot TCP context exchange,
  but not persistent live multi-user editing.
- The legacy `aeon-send` migration path sends full snapshots unless callers use
  the delta API directly.
- Daemon `run` records a paused VM; `resume` executes during the request rather
  than scheduling background VM execution.

## Docs

- [ISA](docs/ISA.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Forth prototype](docs/FORTH.md)
- [Language choice](docs/LANGUAGE_CHOICE.md)
- [Known limitations](KNOWN_LIMITATIONS.md)
