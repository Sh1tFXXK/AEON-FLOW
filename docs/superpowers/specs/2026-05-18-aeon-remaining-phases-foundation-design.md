# AEON Remaining Phases Foundation Design

Date: 2026-05-18

## Goal

Complete the remaining AEON phases as architecture-safe foundations that are usable, testable, and honest about platform limits.

The completed state for this slice is not "all operating systems and apps are deeply hooked." The completed state is:

- AEON has typed domain foundations for credentials, data bridge ingress, operation context, multi-account profiles, and query summaries.
- Each foundation has one state owner and narrow APIs.
- External integrations can attach to those APIs without redesigning capture storage or the web server.
- Security-sensitive features are either implemented with real cryptography or explicitly left as sealed metadata only. No fake vault.

## Current Baseline

Already implemented:

- `aeon-capture` stores captured content in `CaptureStore`.
- `aeon-capture` projects captures into append-only `AeonEvent` records.
- `aeon-sync` exposes `/api/events` and `/api/events/:id`.
- Android can share content into AEON and discover the desktop endpoint.
- Desktop already monitors clipboard, screenshots, files, and selected app state.

Observed pressure points:

- `aeon-sync/src/server.rs` is already a large integration shell. New domains must live in focused modules.
- `CaptureKind` is generic enough for SMS/email/photos through typed metadata; no new capture storage layer is needed.
- `aeon-store` already owns low-level identity/account primitives, but not application account registries or credential vaults.

## Architecture

### Module Ownership

New modules live in `aeon-sync` unless they are pure capture-domain helpers:

- `bridge`: data bridge payload validation and conversion to `CaptureEntry`.
- `operation_context`: current task, scratch pad, clipboard handoff, and AI sessions.
- `account_profiles`: managed external accounts and browser profile launch plans.
- `vault`: encrypted credential vault.
- `query`: deterministic event/capture query summaries.

`server.rs` remains an HTTP composition layer. It may register routes and pass `AppState`, but it must not own domain rules for the new systems.

### State Owners

Each mutable JSON-backed state file has exactly one owner:

- `ContextStore` owns `~/.aeon/context.json`.
- `AccountProfileStore` owns `~/.aeon/account-profiles.json`.
- `CredentialVaultStore` owns `~/.aeon/vault.json`.

Background workers can only create typed payloads and call these owners. UI handlers cannot mutate internal fields directly.

### Capture Bridge

Bridge ingress converts platform-specific data into normal captures:

- SMS becomes `CaptureKind::Text` with `bridge.kind=sms`, sender, direction, provider timestamp, and optional verification code metadata.
- Email becomes `CaptureKind::Conversation` or `CaptureKind::Text` with sender, subject, label, provider timestamp, and message id.
- Mobile photos use existing image capture paths, enriched with `bridge.kind=photo`, album/source app, and screenshot flag.

Verification code extraction is pure and deterministic. It supports Chinese and English code patterns and does not auto-fill browsers directly. Browser fill remains a future consumer of the context/event API.

### Operation Context

Operation context is a small typed document:

- current task
- shared clipboard
- scratch pad
- resumable AI sessions

It is not the event log. Durable history stays in `EventLog` and `CaptureStore`; context only represents the latest operational state.

### Multi-Account Foundation

The account profile registry stores:

- provider/account identity
- user label
- optional credential reference
- browser profile directory
- sharing policy

Chrome launch support is represented as a deterministic launch plan first. Actually spawning Chrome is intentionally separate because process launch is platform and user-policy sensitive.

### Vault Foundation

Vault entries store encrypted secret bytes plus non-secret metadata:

- id
- label
- kind
- domains
- auto-fill preference
- last-used timestamp

The vault must use:

- password-based key derivation with per-vault salt
- authenticated encryption with per-entry nonce
- no plaintext secret persistence

The server may expose metadata lists and encrypted entry creation. It must not return decrypted secrets unless the vault has an explicit unlock session in a later slice. This avoids a half-secure password manager.

### Query Foundation

Natural language AI querying is not implemented as a fake LLM parser in this slice. Instead:

- typed query summaries search existing captures/events deterministically
- the response shape is stable enough for a future local LLM or API-backed parser
- event and capture filters remain typed

## API Shape

Routes are added as narrow domain endpoints:

- `POST /api/bridge/sms`
- `POST /api/bridge/email`
- `GET /api/context`
- `POST /api/context/task`
- `POST /api/context/clipboard`
- `POST /api/context/scratch`
- `POST /api/context/ai-session`
- `GET /api/accounts`
- `POST /api/accounts`
- `POST /api/accounts/:id/browser-plan`
- `GET /api/vault/entries`
- `POST /api/vault/entries`
- `POST /api/query`

Every endpoint delegates to its module. Payload structs are typed and do not use positional arrays.

## Non-Goals

Not included in this slice:

- OS-level keylogging.
- Network proxy capture.
- Microphone capture.
- Browser extension packaging.
- WeChat multi-instance VM workarounds.
- iOS SMS capture.
- Returning decrypted vault secrets through HTTP.
- LLM-backed natural language query planning.

These require explicit user-facing consent flows, platform-specific permissions, and separate threat modeling.

## Security Boundaries

- Bridge data enters through explicit API calls or existing Android share flows.
- Vault secret material is encrypted before it is persisted.
- Vault metadata is non-secret but still local-only by default.
- Auto-fill is not automatic until there is a user-visible approval path.
- Context data may contain sensitive clipboard/task content; it is stored locally under `~/.aeon`.

## Completion Criteria

- Focused modules and tests exist for bridge, operation context, account profiles, vault, and query.
- `aeon-sync` compiles without expanding `server.rs` into the owner of these domains.
- Public docs describe implemented capabilities and explicit limits.
- Tests cover typed state transitions, bridge conversion, vault encryption round trips, and query filtering.
