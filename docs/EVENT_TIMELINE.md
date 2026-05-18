# AEON Event Timeline

This document describes the Phase 1 event timeline foundation. It is deliberately small: capture data is already stored by CID, and this layer records typed, append-only event facts that can be queried by time or id.

## Scope

Implemented:

- A typed `AeonEvent` projection for capture records.
- Deterministic `EventId` values derived from capture identity and CID.
- A local append-only JSONL event log at `~/.aeon/events.jsonl`.
- Capture engine wiring that appends one event only after the capture store write succeeds.
- HTTP read APIs in `aeon-sync`.

Not implemented in this phase:

- Screen/OCR, keyboard, network, audio, or credential capture.
- Entity graph aggregation.
- AI natural-language query.
- Cross-device event-log merge or conflict resolution.

Those are later layers. They should build on this event stream instead of bypassing it.

## Ownership

`CaptureStore` owns raw capture bytes and capture metadata.

`EventLog` owns append-only event history. It only stores event facts, not raw captured bytes.

`CaptureEngine` owns the mutation pipeline:

1. enrich the incoming `CaptureEntry`;
2. write it to `CaptureStore`;
3. project it into `AeonEvent`;
4. append it to `EventLog`;
5. broadcast the capture entry to live subscribers.

The HTTP API reads from `EventLog`; it does not mutate event state.

## Event Model

Core types live in `aeon-capture`:

- `EventId`: 32-byte stable event id with hex parsing and formatting.
- `AeonEvent`: event envelope with `id`, `ts`, `kind`, `source`, `device`, and `identity`.
- `EventKind::CaptureAdded`: projection of an accepted capture.
- `CaptureEvent`: capture-specific event payload containing CID, capture kind, title, summary, size, mime, and typed metadata.
- `EventSource`: identifies whether the event came from local capture or relay import.

The event projection intentionally excludes raw capture bytes. Consumers should dereference the CID through existing capture APIs when they need content.

## Storage

`EventLog` writes one JSON event per line:

```text
~/.aeon/events.jsonl
```

Reads are tolerant of corrupt lines so one bad record does not make the whole timeline unreadable. Query results are returned newest-first.

Supported query fields:

- `from`: optional inclusive Unix millisecond lower bound.
- `to`: optional inclusive Unix millisecond upper bound.
- `limit`: result limit.

The server API defaults `limit` to `100`, rejects `0`, and caps requests at `500`.

## HTTP API

`aeon-sync` exposes:

```http
GET /api/events
GET /api/events?from=1779000000000&to=1779086400000&limit=100
GET /api/events/:id
```

List responses contain:

```json
[
  {
    "id": "32-byte-event-id-as-hex",
    "ts": 1779086400000,
    "kind": { "CaptureAdded": { "...": "..." } },
    "source": { "LocalCapture": { "...": "..." } },
    "device": "16-byte-device-id-as-hex",
    "identity": "32-byte-identity-id-as-hex"
  }
]
```

`GET /api/events/:id` returns:

- `200` with one event when found;
- `400` for invalid event ids;
- `404` when the id is valid but absent.

## Verification

Run the focused crates:

```powershell
cd aeon-capture
cargo test

cd ..\aeon-sync
cargo test
```

Formatting should be checked per crate because the repository root is not a Cargo workspace:

```powershell
cd aeon-capture
cargo fmt -- --check

cd ..\aeon-sync
cargo fmt -- --check
```

## Design Constraints

- No global event singleton.
- No raw bytes in event records.
- No direct API mutation of `EventLog`.
- No stringly-typed event kind branching in domain code.
- No broad capture expansion until the event stream is stable.

