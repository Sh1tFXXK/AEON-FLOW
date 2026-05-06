# Known Limitations

- Heap allocation is a bump allocator with no free operation.
- Heap size is hardcoded to 1MB.
- `StoreMem` writes one byte only.
- Syscalls only cover `open`, `read`, `write`, and `close`.
- VFS is in-memory and serialized in snapshots; it has no host filesystem passthrough.
- Forth is the selected language prototype, but it is intentionally minimal.
- JIT currently targets the small VM instruction subset used by the benchmark path.
- JIT machine-code cache is not serialized and is rebuilt after restore or migration.
- Step 12 P2P sync implements the event-set CRDT, bloom filter, and missing-event calculation, but not a continuously running network gossip task.
- Step 13 incremental snapshots are implemented as `SnapshotDelta`; the legacy `aeon-send` TCP path still sends full snapshots unless a caller opts into the delta API.
- Step 14 daemon commands are checkpoint-oriented: `run` records a paused VM and `resume` executes it during the request rather than scheduling background VM execution.
- Daemon recovery is local to the configured state directory and Unix socket path.
- Console collaboration merges serialized `SharedContext` files; it is not yet a live multi-user transport.
- `aeon-store` media classification uses MIME/extension only; image dimensions and audio/video durations are not parsed yet.
