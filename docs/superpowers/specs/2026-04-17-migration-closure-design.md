# AEON VM Snapshot Format And Migration Closure Design

## Goal

Complete the first usable migration milestone for `aeon-vm` by:

- unifying snapshot file and wire formats
- keeping legacy snapshot reads working
- adding a shared TCP protocol layer
- extending `aeon-run` with sender behavior
- turning `aeon-recv` into a parameterless receiver that can fetch missing programs

This stage ends when a partially executed program can migrate from `aeon-run --send` to `aeon-recv`, automatically transfer the missing program, resume execution, and produce the same final result as a local uninterrupted run.

## Current State

The current repository already has the core pieces for the feature, but they do not form a closed migration loop:

- `src/main.rs` runs programs locally, can snapshot to disk, and can restore from a local snapshot file
- `src/bin/aeon-recv.rs` listens on TCP and can restore a received snapshot only if the matching program is preloaded locally
- `src/snapshot.rs` uses two different formats:
  - `to_bytes()/from_bytes()` use `checksum + bincode(payload)`
  - `save()/load()` use raw `bincode`
- there is no shared transport protocol module
- there is no sender path in `aeon-run`
- there is no request-response flow for fetching a missing program

Because of that split, migration only works in a narrow manual setup and snapshot persistence is format-inconsistent.

## Approved Decisions

The following decisions were explicitly chosen during brainstorming and are fixed for this stage:

1. Sender behavior stays inside `aeon-run`
2. Receiver starts without a required program argument
3. If the receiver lacks the program, it requests it from the sender automatically
4. New snapshots use one unified format
5. Legacy snapshot files remain readable
6. The implementation uses a minimal framed protocol instead of sending an all-in-one bundle

## Scope

This design covers exactly one milestone:

- shared transport protocol
- unified snapshot serialization
- sender-side migration in `aeon-run`
- receiver-side program fetch and resume in `aeon-recv`
- tests for compatibility and migration closure

This design does not cover:

- heap or memory model changes
- snapshot editing
- session/collaboration layers
- multiple migration attempts per connection
- daemonization or background workers
- configurable ports or persistent program caches
- protocol version negotiation

## Architecture

The implementation adds one shared module and updates two existing CLIs:

- `src/protocol.rs`
  owns framed message encoding and decoding, plus protocol message constants
- `src/snapshot.rs`
  becomes the single serialization authority for both file and wire snapshots
- `src/main.rs`
  gains sender behavior behind `--send`
- `src/bin/aeon-recv.rs`
  becomes a pure receiver service that can recover from an empty `ProgramStore`

The migration flow stays deliberately simple: one TCP connection, one snapshot transfer, an optional program fetch, one resume-to-completion, then a terminal response.

## Protocol Design

### Frame Format

Each protocol message uses the same outer frame:

```text
[1 byte msg_type][8 bytes big-endian payload_len][payload bytes]
```

This keeps the transport deterministic and avoids duplicated ad hoc `read_exact` logic in sender and receiver.

### Message Types

The protocol for this stage uses five message types:

- `SNAPSHOT = 0x01`
  Payload is the unified snapshot byte format
- `NEED_PROGRAM = 0x02`
  Payload is exactly 32 bytes of `ProgramId`
- `PROGRAM = 0x03`
  Payload is the serialized `Program`
- `OK = 0x04`
  Payload is empty
- `ERROR = 0x05`
  Payload is a UTF-8 error string

No other protocol messages are needed for this stage.

### Sender To Receiver Flow

The sender flow is:

1. run the target program up to `--snap-at N`
2. if execution already halted, do not migrate
3. capture `Snapshot`
4. connect to receiver
5. send `SNAPSHOT`
6. wait for a reply
7. if reply is `NEED_PROGRAM`, verify the requested `ProgramId` matches the currently running program and send `PROGRAM`
8. wait for final `OK` or `ERROR`
9. exit success on `OK`, fail on any unexpected or error response

The sender never sends a program unless it is explicitly requested.

### Receiver Flow

The receiver flow is:

1. listen on `0.0.0.0:9999`
2. accept one incoming connection
3. read the first message and require it to be `SNAPSHOT`
4. try to restore the snapshot against the local `ProgramStore`
5. if the program is missing, send `NEED_PROGRAM(snapshot.program_id())`
6. require the next message to be `PROGRAM`
7. deserialize the program, add it to the store, and retry restore
8. run to completion
9. print result information
10. reply `OK`

Any invalid message order, deserialize failure, restore failure, or runtime error sends `ERROR` and terminates the session.

## Snapshot Compatibility Strategy

The canonical snapshot format for all newly written snapshots becomes:

```text
[32-byte blake3 checksum of payload][bincode(snapshot payload)]
```

That is already what `Snapshot::to_bytes()` produces, so the change is to make every other path align to it.

### Required Behavior

- `Snapshot::to_bytes()` remains the canonical serializer
- `Snapshot::save(path)` writes `to_bytes()`
- `Snapshot::from_bytes(bytes)` accepts:
  - new canonical format with checksum validation
  - old raw `bincode` snapshot bytes as a fallback
- `Snapshot::load(path)` reads file bytes and passes them through the same compatibility parser

### Compatibility Rules

The parser order is fixed:

1. if the byte slice is at least 32 bytes, try the canonical `checksum + payload` path first
2. if checksum validation and payload deserialization succeed, return that snapshot
3. otherwise, fall back to deserializing the entire byte slice as the legacy raw `bincode` snapshot
4. if both fail, return an invalid-data error

This gives "wide read, narrow write" behavior:

- legacy files keep working
- all newly written files and network payloads converge to one format

## CLI Design

### `aeon-run`

The sender interface is:

```bash
aeon-run <file.aeon> --snap-at <n> --send <host:port>
```

Rules:

- `--send` requires `--snap-at`
- `--restore` and `--send` are mutually exclusive in this stage
- if bounded execution halts before the snapshot point, no network migration occurs
- if bounded execution does not halt, the VM migrates instead of writing only a local `.snap`
- local snapshot files continue to be produced when `--snap-at` is used without `--send`

Observable output should stay simple:

- bounded run result
- migration target
- protocol outcome
- non-zero registers when execution remains local or after local restore

The sender does not continue executing locally after a successful migration. Ownership transfers to the receiver for this stage.

### `aeon-recv`

The receiver interface becomes:

```bash
aeon-recv
```

Rules:

- listens on `0.0.0.0:9999`
- starts with an empty `ProgramStore`
- automatically requests the program when restore fails because the program is missing
- runs the restored VM to completion
- prints final step count and non-zero registers

The CLI no longer requires `<program.aeon>` as an argument.

## Error Handling

The protocol must fail loudly and deterministically.

Sender-side error cases:

- missing `--snap-at` when `--send` is present
- invalid `host:port`
- connect failure
- first reply is not `NEED_PROGRAM`, `OK`, or `ERROR`
- receiver asks for a different `ProgramId`
- program serialization failure
- receiver returns `ERROR`

Receiver-side error cases:

- first message is not `SNAPSHOT`
- snapshot deserialization fails
- legacy or canonical snapshot parsing both fail
- missing program and second message is not `PROGRAM`
- program deserialization fails
- restored VM runtime fails

All receiver-side protocol failures return `ERROR` before closing the connection when possible.

## Testing Strategy

This stage needs tests at three levels: snapshot compatibility, protocol correctness, and migration closure.

### Snapshot Compatibility Tests

Add tests for:

- new bytes round-trip through `to_bytes()/from_bytes()`
- legacy raw `bincode` snapshot bytes remain readable through `from_bytes()`
- `save()` writes the same bytes as `to_bytes()`
- `load()` accepts both newly written files and legacy raw files

### Protocol Tests

Add tests for:

- framed message round-trip for each supported message type
- correct handling of payload lengths
- invalid message order producing a protocol error

These can live as unit tests around `protocol.rs` and small helper functions.

### Migration Closure Tests

Add an end-to-end test that:

1. creates a program locally on the sender side
2. runs it partway
3. sends only the snapshot first
4. starts the receiver with an empty `ProgramStore`
5. forces the receiver to request the missing program
6. sends the program
7. resumes and runs to completion
8. compares final state against a local uninterrupted run

This is the critical proof that the chosen stage is complete.

## File-Level Change Plan

The design expects the following file changes:

- Create: `aeon-vm/src/protocol.rs`
- Modify: `aeon-vm/src/lib.rs`
  export the new protocol module
- Modify: `aeon-vm/src/snapshot.rs`
  unify serialization and compatibility parsing
- Modify: `aeon-vm/src/main.rs`
  add sender flags and migration flow
- Modify: `aeon-vm/src/bin/aeon-recv.rs`
  remove required program argument and add program-fetch handshake
- Modify: `aeon-vm/tests/tests.rs`
  add compatibility and migration tests

No unrelated refactors are part of this stage.

## Non-Goals And Deferred Work

The following are intentionally deferred:

- making the receiver serve multiple clients in one process lifetime
- configurable ports or addresses
- persistent program cache on disk
- integrity checks for `Program` bytes beyond current deserialize success
- protocol version fields
- richer response payloads such as final register dumps inside `OK`

These are valid future tasks, but they are not required to close the first migration milestone.

## Success Criteria

This design is complete only if all of the following are true:

- `aeon-run <file.aeon> --snap-at N --send host:port` migrates a non-halted execution
- `aeon-recv` can start with no preloaded program
- receiver requests the program only when needed
- the transferred program matches the snapshot `ProgramId`
- new snapshot files use the same bytes as network snapshots
- legacy snapshot files still load
- migration produces the same final VM result as uninterrupted local execution
- `cargo test` remains fully passing
