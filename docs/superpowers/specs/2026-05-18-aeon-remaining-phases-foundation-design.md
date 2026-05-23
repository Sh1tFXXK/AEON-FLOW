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
- Android can start an SMS observer service that reads `content://sms` after
  `READ_SMS` is granted and posts typed SMS bridge payloads to the desktop.
- Desktop already monitors clipboard, OS foreground-window activity, screenshots as image artifacts, files, and selected app state.

Observed pressure points:

- `aeon-sync/src/server.rs` is already a large integration shell. New domains must live in focused modules.
- `CaptureKind` is generic enough for SMS/email/photos through typed metadata; no new capture storage layer is needed.
- `aeon-store` already owns low-level identity/account primitives, but not application account registries or credential vaults.

## Architecture

### Module Ownership

New modules live in `aeon-sync` unless they are pure capture-domain helpers:

- `bridge`: data bridge payload validation and conversion to `CaptureEntry`.
- `email_sync`: email account config, provider-worker import, and cursor state.
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
- `EmailSyncStore` owns `~/.aeon/email-sync.json`.

Background workers can only create typed payloads and call these owners. UI handlers cannot mutate internal fields directly.

### Capture Bridge

Bridge ingress converts platform-specific data into normal captures:

- SMS becomes `CaptureKind::Text` with `bridge.kind=sms`, sender, direction, provider timestamp, and optional verification code metadata.
- Email becomes `CaptureKind::Conversation` or `CaptureKind::Text` with sender, subject, label, provider timestamp, and message id.
- Mobile photos use existing image capture paths, enriched with `bridge.kind=photo`, album/source app, and screenshot flag.

Email sync stores typed account configs for IMAP/Gmail/Outlook-style providers
and cursor state per account. Provider workers can submit already-fetched
messages through a narrow import endpoint. AEON can also directly call Gmail API
or Microsoft Graph for configured OAuth-backed API accounts. Expired or
near-expiry OAuth access tokens are renewed with the vault entry's refresh-token
grant config and persisted back into the encrypted vault before provider fetch.
For IMAP accounts, AEON uses a vault-backed password credential, connects over
TCP or TLS, selects the configured mailbox, fetches recent UIDs, and normalizes
RFC822 messages through the same import path. All paths deduplicate by message
id, advance the cursor, and store new messages through the normal capture
engine.

Screen and input context use operating-system or application events. They do not use OCR, and raw keyboard hooks are not an accepted bridge source.

Verification code extraction is pure and deterministic. It supports Chinese and English code patterns. Browser fill is handled by the local Chromium extension through the loopback-only latest-code endpoint and content-script fill message.

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

The launch plan may include a web target URL for "open this page under another
account" workflows. Only `http://` and `https://` targets are accepted. The plan
also carries the account label and credential reference so UI/native consumers
can connect the browser profile to vault metadata without decrypting secrets.

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

The server may expose metadata lists, encrypted entry creation, derived TOTP
codes for `Totp` entries, and loopback-only password fill through a short-lived
unlock session. The TOTP endpoint returns only the six-digit code, expiry time,
and period. Password fill returns at most one `Password` entry, requires
`auto_fill=true`, requires a matching domain, and requires an unexpired in-memory
session created by `POST /api/vault/unlock`.

### Query Foundation

Natural language AI querying is not implemented as a fake LLM parser in this slice. Instead:

- "today activity" questions are answered deterministically from capture timestamps and metadata
- typed query summaries search existing captures/events deterministically
- structured LLM query-plan JSON can be parsed and executed as typed filters
- optional OpenAI-compatible and Ollama-compatible planner endpoints can produce the typed query plan before local execution
- unreachable, invalid, or empty planner responses fall back to deterministic local query behavior
- event and capture filters remain typed

## API Shape

Routes are added as narrow domain endpoints:

- `POST /api/bridge/sms`
- `POST /api/bridge/email`
- `POST /api/bridge/browser-page`
- `GET /api/bridge/verification-code/latest`
- `GET /api/email/accounts`
- `POST /api/email/accounts`
- `POST /api/email/accounts/:id/import`
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
- `POST /api/vault/totp/:id`
- `POST /api/vault/unlock`
- `POST /api/vault/fill`
- `POST /api/email/accounts/:id/sync`
- `POST /api/query`
- `POST /api/query/structured`

Every endpoint delegates to its module. Payload structs are typed and do not use positional arrays.

## Non-Goals

Not included in this slice:

- OS-level keylogging.
- Network proxy capture.
- Microphone capture.
- Browser extension store packaging/signing and native messaging.
- WeChat multi-instance VM workarounds.
- iOS SMS capture.
- Provider-specific IMAP compatibility hardening beyond the baseline direct
  UID SEARCH/FETCH connector.
- Returning decrypted vault secrets through non-loopback HTTP or without an
  explicit unlock session.
- Remote browser process spawning over unauthenticated HTTP.
- Streaming planner responses and provider-specific tool-calling schemas.

These require explicit user-facing consent flows, platform-specific permissions, and separate threat modeling.

## Security Boundaries

- Bridge data enters through explicit API calls or existing Android share flows.
- Android SMS data enters only after the user starts the SMS bridge and grants
  the Android `READ_SMS` permission.
- Vault secret material is encrypted before it is persisted.
- Vault metadata is non-secret but still local-only by default.
- Browser password fill requires a user-visible extension popup unlock action,
  an unexpired session, and a matching `auto_fill=true` password entry.
- Context data may contain sensitive clipboard/task content; it is stored locally under `~/.aeon`.

## Completion Criteria

- Focused modules and tests exist for bridge, email sync, operation context, account profiles, vault, and query.
- `aeon-sync` compiles with `server.rs` reduced to AppState and router ownership; route handlers live in focused `server/` modules under 400 lines each.
- `aeon-vm` snapshot editor heap operations no longer return Step 3 placeholder errors; heap byte/range/string/u64/top edits are reversible patch entries.
- `aeon-vm` daemon commands are available on non-Unix targets through localhost TCP instead of Unix-socket-only stubs.
- Public docs describe implemented capabilities and explicit limits.
- Tests cover typed state transitions, bridge conversion, email sync cursor import, Gmail/Outlook provider normalization, direct IMAP protocol fetch and vault-backed import, OAuth refresh-token renewal and vault persistence, vault encryption round trips, query filtering, optional planner request/response handling, and structured LLM query-plan execution.
