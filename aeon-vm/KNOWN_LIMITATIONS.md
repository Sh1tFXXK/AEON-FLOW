# Known Limitations

- Heap allocation is a bump allocator with no free operation.
- Heap size is hardcoded to 1MB.
- `StoreMem` writes one byte only.
- Step 12 P2P sync currently implements the event-set CRDT, bloom filter, and
  missing-event calculation, but not a continuously running network gossip task.
- Step 13 incremental snapshots are implemented as `SnapshotDelta`; the legacy
  `aeon-send` TCP path still sends full snapshots unless a caller opts into the
  delta API.
- Step 14 daemon commands are checkpoint-oriented: `run` records a paused VM and
  `resume` executes it during the request rather than scheduling background VM
  execution.
