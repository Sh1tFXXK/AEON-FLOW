# Known Limitations

- Heap allocation is a bump allocator with no free operation.
- Heap size is hardcoded to 1MB.
- `StoreMem` writes one byte only.
- Syscalls only cover `open`, `read`, `write`, and `close`.
- VFS is in-memory and serialized in snapshots; it has no host filesystem passthrough.
- Forth is the selected language prototype, but it is intentionally minimal.
- JIT currently targets the small VM instruction subset used by the benchmark path.
- JIT machine-code cache is not serialized and is rebuilt after restore or migration.
- Step 12 event-set sync implements the CRDT, bloom filter, and missing-event calculation. Content-addressed CID sync now has a long-running `sync --serve` listener, but the VM event-set CRDT is still a deterministic reconciliation primitive rather than a separate background event daemon.
- Step 13 incremental snapshots are implemented as `SnapshotDelta`; the legacy `aeon-send` TCP path still sends full snapshots unless a caller opts into the delta API.
- Step 14 daemon commands are checkpoint-oriented: `run` records a paused VM and `resume` executes it during the request rather than scheduling background VM execution.
- Daemon recovery is local to the configured state directory. Unix uses the configured socket path; non-Unix command transport uses localhost TCP by default.
- Console collaboration has a typed `SharedContext::merge_from` owner API and a tested one-shot TCP context exchange. It is not yet a persistent live multi-user editing service.
- `aeon-store` media classification uses MIME/extension only; image dimensions and audio/video durations are not parsed yet.
