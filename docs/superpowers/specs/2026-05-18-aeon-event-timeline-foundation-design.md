# AEON Event Timeline Foundation Design

## Goal

Build the first foundation for the larger AEON personal operating layer by adding an append-only event timeline that wraps existing capture records instead of replacing them.

This stage turns "things AEON captured" into "things that happened over time" while preserving the current CIDStore, `CaptureEntry`, Relay, Android sharing, and desktop UI behavior.

## Current State

The repository already has a working capture stack:

- `aeon-capture` owns `CaptureEntry`, `CaptureKind`, `CaptureSource`, `CaptureMetadata`, and `CaptureEngine`
- `aeon-capture::CaptureStore` stores raw bytes in `CIDStore` and metadata in `capture-index.json`
- `aeon-sync` exposes desktop HTTP APIs and the static UI
- Relay and Android sharing already import remote text, files, and images into the same capture stream
- app capture modules already know how to capture browsers, Claude Desktop, VS Code, terminals, processes, and AEON VM snapshots

The missing piece is a typed event timeline. Today the capture index answers "what content do I have?" but not "what happened, in what order, from which device, with what event type?"

## Approved Direction

The first event system uses the already approved approach:

- Event wraps or references existing `CaptureEntry`
- Event does not replace `CaptureEntry`
- Event does not duplicate raw captured bytes
- Event is append-only
- Event is the temporal layer; CIDStore remains the content layer
- implementation starts with Phase 1 only, then proceeds through the larger AEON roadmap in later phases

## Full Project Completion Strategy

The complete AEON vision is valid, but it is too large and too sensitive for a single implementation change. It will be completed as isolated phases:

1. Event timeline foundation
2. realtime Event timeline UI and query API
3. typed capture expansion for existing OS/application sources
4. Android SMS verification-code bridge
5. context bus for clipboard, current task, and AI sessions
6. encrypted credential vault without browser injection
7. browser extension for URL capture and controlled fill actions
8. multi-account Chrome profile management and unified search

Each phase must have its own design, plan, tests, and verification before the next phase begins.

## Scope For This Stage

This stage adds only the Event timeline foundation:

- typed `AeonEvent` model
- append-only event log
- automatic `CaptureEntry` to `AeonEvent` projection inside the capture pipeline
- list and range-query APIs for recent events
- tests for event projection, append-only persistence, and chronological reads

This stage does not add:

- pixel OCR-derived screen text
- raw keyboard hooks
- HTTP proxy capture
- audio capture
- credential storage
- browser credential injection
- SMS permission handling
- AI summarization
- entity graph inference
- RocksDB or Tantivy
- background workers that directly mutate UI-visible state

## Architecture

The design keeps three layers separate:

- `CaptureEntry`: content that AEON captured, addressed by CID
- `AeonEvent`: typed record that something happened at a timestamp
- API/UI projection: read-only rendering of recent timeline events

`CaptureEngine` remains the single mutation pipeline for capture-visible state. When a capture is accepted and stored successfully, it appends one `AeonEvent::CaptureAdded` record to the event log. The event log is not allowed to mutate capture records.

The event log is owned by a small `EventLog` type. It is append-only on disk and exposes read methods that return typed events. The HTTP layer reads events through this owner rather than reading the file directly.

## Event Model

Phase 1 should create a deliberately small event model:

```rust
pub struct AeonEvent {
    pub id: EventId,
    pub ts: u64,
    pub kind: EventKind,
    pub source: EventSource,
    pub device: [u8; 16],
    pub identity: [u8; 32],
}

pub struct EventId(pub [u8; 32]);

pub enum EventKind {
    CaptureAdded(CaptureEvent),
}

pub struct CaptureEvent {
    pub cid: CID,
    pub capture_kind: CaptureKind,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub app_name: Option<String>,
    pub file_path: Option<String>,
    pub url: Option<String>,
    pub size: usize,
    pub mime: String,
}

pub enum EventSource {
    LocalCapture(CaptureSource),
    RelayImport { device_name: String },
}
```

The event ID is derived from stable event fields with `blake3`, not from a global mutable counter. This preserves append-only semantics and avoids introducing a new global ID allocator.

## Storage Design

Phase 1 uses a JSONL append-only log at:

```text
~/.aeon/events.jsonl
```

Each line is one serialized `AeonEvent`.

This choice is intentional:

- easy to inspect during early development
- easy to test without native database dependencies
- avoids Windows RocksDB build risk
- keeps the first event layer boring and reversible
- leaves room to add RocksDB/Tantivy later behind the same `EventLog` interface

The log never stores raw capture bytes. Raw bytes stay in `CIDStore`.

If an event line is corrupt, reads should skip only that line and continue. Writes must remain strict: serialization or filesystem write errors return an error to the caller.

## File Boundaries

Expected files for Phase 1:

- Create `aeon-capture/src/event.rs`
  - owns `AeonEvent`, event enums, event ID creation, and `CaptureEntry` projection
- Create `aeon-capture/src/event_log.rs`
  - owns append-only JSONL persistence and query methods
- Modify `aeon-capture/src/lib.rs`
  - exports the new event modules
- Modify `aeon-capture/src/engine.rs`
  - appends an event only after capture storage succeeds
- Modify `aeon-sync/src/main.rs`
  - opens the event log under the same AEON home as the capture index
- Modify `aeon-sync/src/server.rs`
  - exposes read-only `/api/events` and `/api/events/:id` endpoints
- Add tests in `aeon-capture` and `aeon-sync` where the behavior lives

No unrelated refactors are part of this stage.

## Data Flow

The normal capture path becomes:

1. source creates a `CaptureEntry`
2. `CaptureEngine::capture` stamps identity and device
3. `CaptureEngine::capture` enriches the entry
4. `CaptureStore::put` writes content and metadata
5. `CaptureEngine::capture` projects the stored capture into `AeonEvent`
6. `EventLog::append` writes one JSONL record
7. `CaptureEngine` broadcasts the existing `CaptureEntry`

The event append comes after capture storage. A failed event append should make `capture` fail because the event timeline is now part of the capture contract. The implementation must avoid writing events for captures that were not stored.

## API Design

Phase 1 adds read-only APIs:

```text
GET /api/events?limit=100
GET /api/events?from=<unix_ms>&to=<unix_ms>&limit=100
GET /api/events/:id
```

The API returns typed event payloads and does not expose raw captured bytes. Existing raw-content APIs stay under `/api/entry/:cid/raw`.

There is no mutation endpoint for events in this phase.

## Error Handling

Event write errors should propagate out of `CaptureEngine::capture`.

Event read behavior:

- missing log file returns an empty list
- malformed JSONL line is skipped
- invalid query parameters return `400`
- unknown event ID returns `404`

The design favors correctness for writes and resilience for reads.

## Testing Strategy

Tests must prove:

- projecting a `CaptureEntry` creates exactly one `CaptureAdded` event
- event ID generation is deterministic for the same event fields
- `EventLog::append` writes one JSONL line per event
- reading the log returns events newest-first for list queries
- range queries include only events whose timestamp is inside the requested range
- corrupt lines do not prevent valid later events from loading
- `CaptureEngine::capture` does not append an event if capture storage fails

The first implementation should run at least:

```powershell
cargo test -p aeon-capture
cargo test -p aeon-sync
```

If the workspace does not use a root Cargo workspace, run the equivalent commands inside each crate directory.

## Non-Goals

The following are explicitly deferred:

- replacing `CaptureEntry`
- changing existing CID calculation
- full-text search
- entity graph inference
- AI natural-language queries
- encrypted credential storage
- automatic browser fill
- global keyboard capture or pixel OCR-derived screen text
- all-device conflict resolution
- public Relay authentication

These are later phases, not hidden requirements for Phase 1.

## Success Criteria

This phase is complete when:

- every successful capture creates one typed event
- event history survives process restart
- `/api/events` returns recent events without reading raw capture bytes
- existing `/api/entries` and capture UI behavior continue to work
- tests cover projection, persistence, query behavior, and failure behavior
- no new global mutable state or cross-module mutation path is introduced
