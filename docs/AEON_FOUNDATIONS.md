# AEON Foundations

Date: 2026-05-18

This document records the implemented AEON foundations that sit above the original capture stream.

## Implemented Foundations

### Data Bridge

AEON now accepts typed bridge payloads and stores them through the existing capture engine.

Endpoints:

- `POST /api/bridge/sms`
- `POST /api/bridge/email`
- `POST /api/bridge/browser-page`

SMS payload:

```json
{
  "message_id": "sms-1",
  "address": "10086",
  "body": "Your code is 476291",
  "received_at": 1771000000000,
  "direction": "Incoming"
}
```

Email payload:

```json
{
  "message_id": "email-1",
  "from": "noreply@example.test",
  "to": ["wc@example.test"],
  "subject": "Build finished",
  "body_preview": "AEON build completed successfully",
  "received_at": 1771000000100,
  "labels": ["inbox"]
}
```

Browser page payload:

```json
{
  "url": "https://example.test/private",
  "title": "Private dashboard",
  "captured_at": 1771000000222,
  "account_id": "google-work",
  "tab_id": 42
}
```

SMS verification code extraction is deterministic and tested in both Rust and Android Kotlin.
Browser page bridge capture is URL/title metadata only; it does not fetch
private page content through the desktop service.

The bridge also exposes loopback-only
`GET /api/bridge/verification-code/latest`. It returns the most recent
SMS-derived verification code while it is inside its five-minute expiry window,
including only the code, sender address, received timestamp, and expiry
timestamp. The browser extension uses this read-only endpoint from
`127.0.0.1`; LAN clients receive `403 Forbidden`.

### Email Sync Foundation

AEON now has a typed email sync state store for IMAP/Gmail/Outlook-style
accounts.

State file:

- `~/.aeon/email-sync.json`

Endpoints:

- `GET /api/email/accounts`
- `POST /api/email/accounts`
- `POST /api/email/accounts/:id/import`
- `POST /api/email/accounts/:id/sync`

Account configs are typed as one of:

- `Imap { host, port, tls, mailbox }`
- `GmailApi { account_id }`
- `OutlookApi { account_id }`

The import endpoint accepts already-fetched messages from a provider worker,
deduplicates by message id, advances the per-account cursor, and converts each
new message into the same `CaptureKind::Text` / `bridge.email` capture records
used by `POST /api/bridge/email`. Imported captures include account id, account
label, provider key, UID, and optional credential reference metadata.

The sync endpoint is loopback-only and uses an unexpired vault unlock session to
read the configured credential. Gmail and Outlook accounts use an `OAuthToken`
vault entry. If the access token is expired or inside the refresh window, AEON
builds a refresh-token grant request from the vault entry, refreshes outside the
vault mutex, and persists the rotated encrypted token before calling the
provider. Gmail API sync lists message ids and fetches metadata headers for each
message; Outlook API sync uses Microsoft Graph messages with a narrow selected
field set.

IMAP accounts use a `Password` vault entry. AEON opens a direct TCP or TLS
connection, logs in, selects the configured mailbox, lists UIDs, fetches recent
RFC822 message bodies, and normalizes parsed headers/body preview into
`FetchedEmailMessage`. The IMAP TLS client uses WebPKI roots and an explicit
rustls crypto provider rather than relying on process-global provider
selection.

Provider responses are normalized to the same typed `FetchedEmailMessage`
structure before cursor deduplication and capture storage.

### Operation Context Bus

AEON now has a single JSON-backed owner for current operational state.

State file:

- `~/.aeon/context.json`

Endpoints:

- `GET /api/context`
- `POST /api/context/task`
- `POST /api/context/clipboard`
- `POST /api/context/scratch`
- `POST /api/context/ai-session`

The context bus stores current task, shared clipboard text, scratch pad text, and resumable AI sessions. It is not a durable event log; durable history remains in capture records and `events.jsonl`.

### Account Profile Registry

AEON now stores external account profiles and browser profile launch plans.

State file:

- `~/.aeon/account-profiles.json`

Endpoints:

- `GET /api/accounts`
- `POST /api/accounts`
- `POST /api/accounts/:id/browser-plan`

Sensitive browser state is isolated by default:

- cookies
- local storage
- passwords

The browser plan endpoint returns launch arguments. It includes account identity,
label, optional credential reference, isolated profile arguments, optional AEON
extension loading, and an optional `http://` or `https://` target URL for
"open this page as another account" workflows.

It does not spawn Chrome directly and does not decrypt credentials.

### Local CLI Account Registry

The `aeon` VM/data CLI now has a small JSON-backed local account registry.

State file:

- `~/.aeon/accounts.json`
- Override with `AEON_ACCOUNT_REGISTRY`

Commands:

- `aeon account <display-name> <public-key-hex>` creates or updates an account
  by public-key-derived account id and persists it locally.
- `aeon accounts` lists local accounts and marks the default account.
- `aeon whoami` prints `AEON_ACCOUNT_ID` when explicitly set; otherwise it
  prints the registry default account id or `unconfigured`.

The first local account becomes the default. Upserting the same public key
replaces the display name without creating a duplicate.

### Credential Vault

AEON now has an encrypted credential vault foundation.

State file:

- `~/.aeon/vault.json`

Endpoints:

- `GET /api/vault/entries`
- `POST /api/vault/entries`
- `POST /api/vault/totp/:id`
- `POST /api/vault/unlock`
- `POST /api/vault/fill`

`POST /api/vault/totp/:id` accepts the vault password and returns the current
six-digit RFC 6238 TOTP code plus expiry metadata for `Totp` entries. The
response does not include the TOTP secret or any decrypted credential payload.

`POST /api/vault/unlock` is loopback-only. It verifies the master password and
creates a short-lived in-memory session that stores the derived vault key, not
the plaintext password. `POST /api/vault/fill` is also loopback-only and accepts
that session id plus a web URL; it returns at most one matching `Password`
credential for entries with `auto_fill=true` and a matching domain.

Security properties:

- PBKDF2-HMAC-SHA256 key derivation.
- Per-vault salt.
- AES-256-GCM authenticated encryption.
- Per-entry nonce.
- Metadata listing does not return decrypted secrets.
- TOTP generation exposes only the derived one-time code, expiry time, and
  period.
- Password fill exposes decrypted username/password only to loopback callers
  holding an unexpired unlock session.

### Query Foundation

AEON now exposes deterministic capture search.

Endpoint:

- `POST /api/query`
- `POST /api/query/structured`

Payload:

```json
{
  "question": "today what did I do",
  "text": "context",
  "kind": "Text",
  "limit": 20
}
```

This is intentionally not a fake LLM parser. It supports typed filters and a
deterministic "today activity" summary for questions such as "today what did I do".
It also exposes a structured query-plan executor for LLM planners. LLM output
can be parsed from strict JSON or fenced JSON and then executed as typed filters:
text, capture kind, time range, and limit.

If configured, `POST /api/query` can call a hosted or local planner before
falling back to deterministic local behavior:

- `AEON_QUERY_PLANNER_URL`
- `AEON_QUERY_PLANNER_MODEL`
- `AEON_QUERY_PLANNER_PROVIDER=openai-compatible|ollama-chat`
- `AEON_QUERY_PLANNER_API_KEY` (optional bearer token)

The planner receives only the question and current Unix millisecond timestamp.
Capture records stay local; invalid or empty plans are ignored.

### Android SMS Bridge

Android now includes:

- `VerificationCodeExtractor`
- `AeonAgent.SmsBridgePayload`
- `AeonAgent.captureSmsResult`
- `SmsBridge`
- `SmsWatcherService`

`SmsWatcherService` registers a `content://sms` observer after the user grants
`READ_SMS`, converts each SMS row into the typed bridge payload, and posts it to
`POST /api/bridge/sms`. The mobile UI exposes this as `Start SMS bridge`.

This is an APK-level implementation. Runtime use still depends on Android device
policy, user permission, and carrier/OEM behavior, so real phone verification is
tracked separately from unit/build verification.

### VM Snapshot Editor

`aeon-vm` snapshot editing now supports heap changes instead of returning the
old Step 3 placeholder error.

Implemented editor operations:

- `set_heap_byte`
- `set_heap_range`
- `set_heap_str`
- `set_heap_u64`
- `set_heap_top`

Heap edits are represented as reversible `Patch::HeapRange` and
`Patch::HeapTop` entries, so the same `PatchSet::apply` and `PatchSet::reverse`
path used for registers, PC, and call-stack edits now covers heap bytes and the
allocation pointer. The editor rejects missing heap snapshots, out-of-range
ranges, and `heap_top` values beyond the heap length.

### VM Daemon Transport

`aeon-vm` daemon commands now work on Windows and other non-Unix targets through
a loopback-only TCP command transport.

Defaults:

- Unix: existing Unix socket command transport.
- Non-Unix: `127.0.0.1:9998`, overrideable with `AEON_DAEMON_ADDR`.

The TCP transport uses the same line-delimited JSON request/response protocol as
the Unix socket path. It binds only to localhost by default and exists for CLI to
daemon control commands such as `ps`, `devices`, `run`, and `migrate`; it is not
a LAN control surface.

### Route Ownership

`aeon-sync/src/server.rs` now owns only shared server state, static index
serving, and the Axum route table. Route handlers are split by domain under
`aeon-sync/src/server/`:

- `devices.rs`
- `entries.rs`
- `files.rs`
- `ingest.rs`
- `process_capture.rs`
- `process_helpers.rs`
- `shared.rs`
- `ws.rs`

Each server route file is under 400 lines after the split. Capture payload
projection and shared metadata helpers live in `shared.rs`; domain modules
continue to mutate state only through `AppState` and the existing store APIs.

### Continuous CID Sync Listener

`aeon-store::SyncEngine` now supports both one-shot and bounded multi-session
content sync listeners. The existing `sync --listen <port|addr>` command still
accepts one peer connection and exits. New long-running service mode is:

```powershell
aeon-data sync --serve 7070
aeon sync --serve 7070
```

For tests, scripts, and controlled smoke runs, pass an explicit session count:

```powershell
aeon-data sync --serve 127.0.0.1:7070 --sessions 2
aeon sync --serve 127.0.0.1:7070 --sessions 2
```

Peers continue to push a specific CID through:

```powershell
aeon-data sync --announce <cid> --peer 127.0.0.1:7070
aeon sync --announce <cid> --peer 127.0.0.1:7070
```

This is content-addressed blob sync. It does not change the VM event-set CRDT
semantics, which remain a deterministic reconciliation layer.

### VM Collaboration Context Exchange

`SharedContext` now owns collaboration merges through `merge_from`. The merge
imports unique attributed patches, messages, and connected sessions, and rejects
contexts from a different VM program before mutating local state. The console
`join` command uses that owner API instead of editing patch/message vectors
directly.

`aeon-vm::collab_transport` adds a bounded one-shot TCP exchange. A client sends
its local context, the listener merges it, and the listener returns its merged
context for the client to merge. This provides a typed network foundation
without introducing a persistent live editing service.

## Explicit Limits

Not implemented as completed integrations:

- OS keylogging.
- Network proxy capture.
- Microphone capture.
- Browser extension store packaging/signing and native messaging.
- Browser-native password import/export.
- Persistent live VM multi-user editing beyond one-shot context exchange.
- Real-provider IMAP compatibility validation beyond the local protocol tests.
- WeChat multi-instance VM workaround.
- iOS SMS capture.
- Streaming planner responses and provider-specific tool-calling schemas.

These require explicit user consent, platform permissions, and separate threat modeling.

## Verification

Focused tests cover:

- bridge conversion and verification-code extraction
- email sync account config persistence and cursor-based deduplicated import
- Gmail/Outlook provider request construction and response normalization
- direct IMAP protocol fetch and sync import with vault-backed password credentials
- loopback/vault-session safety checks for direct email sync
- OAuth refresh-token request construction, response application, and encrypted token persistence
- operation context state ownership
- account profile launch planning
- account profile target-URL switching and credential references
- credential vault encryption and wrong-password failure
- loopback-only vault unlock sessions and domain-scoped password fill
- deterministic query filtering
- optional OpenAI-compatible/Ollama-compatible query planner invocation
- structured LLM query-plan parsing and execution
- Android verification-code extraction
- Android SMS bridge payload conversion
- browser page bridge conversion and HTTP ingestion
- VM snapshot heap editor apply/reverse behavior
- non-Unix daemon JSON command round trip over localhost TCP
- split `aeon-sync` server route modules with all route files under 400 lines
- multi-session CID sync listener and bounded two-session transfer
- VM shared-context merge ownership and one-shot TCP context exchange
