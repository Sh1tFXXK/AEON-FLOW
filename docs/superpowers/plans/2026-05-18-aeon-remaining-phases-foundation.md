# AEON Remaining Phases Foundation Plan

Date: 2026-05-18

Design: `docs/superpowers/specs/2026-05-18-aeon-remaining-phases-foundation-design.md`

## Constraints

- Keep new domain rules outside `aeon-sync/src/server.rs`.
- Add tests before production behavior for each module.
- Prefer typed enums and structs over string dispatch.
- Do not implement OS-level surveillance features without explicit consent and threat modeling.
- Do not expose decrypted credentials through HTTP in this slice.

## Implementation Slices

### 1. Bridge Ingress

Files:

- `aeon-capture/src/bridge.rs`
- `aeon-capture/src/lib.rs`
- `aeon-sync/src/bridge.rs`
- `aeon-sync/src/server.rs`

Tests first:

- verification code extraction handles Chinese, English, and ambiguous text
- SMS payload converts to `CaptureEntry` with typed bridge metadata
- email payload converts to `CaptureEntry` with typed bridge metadata

Behavior:

- add pure `VerificationCode` extraction helper
- add `SmsBridgePayload` and `EmailBridgePayload`
- add `/api/bridge/sms` and `/api/bridge/email`
- capture bridge payloads through the existing `CaptureEngine`

Verification:

- `cargo test -p aeon-capture bridge`
- `cargo test -p aeon-sync bridge`

### 2. Operation Context Bus

Files:

- `aeon-sync/src/operation_context.rs`
- `aeon-sync/src/main.rs`
- `aeon-sync/src/server.rs`

Tests first:

- default context is valid
- updates replace only their owned field
- AI sessions upsert by id
- JSON-backed store round trips

Behavior:

- add `OperationContext`, `TaskContext`, `ClipboardContext`, `AiSessionContext`
- add `ContextStore`
- add `/api/context`
- add update endpoints for task, clipboard, scratch, and AI session

Verification:

- `cargo test -p aeon-sync operation_context`

### 3. Account Profile Registry

Files:

- `aeon-sync/src/account_profiles.rs`
- `aeon-sync/src/main.rs`
- `aeon-sync/src/server.rs`

Tests first:

- account upsert is deterministic by id
- sharing policy defaults isolate sensitive browser state
- Chrome launch plan uses an isolated user data directory and AEON extension path when configured

Behavior:

- add typed provider, account, browser profile, and sharing policy structures
- add JSON-backed `AccountProfileStore`
- add `/api/accounts`
- add `/api/accounts`
- add `/api/accounts/:id/browser-plan`

Verification:

- `cargo test -p aeon-sync account_profiles`

### 4. Credential Vault Metadata and Encryption

Files:

- `aeon-sync/src/vault.rs`
- `aeon-sync/Cargo.toml`
- `aeon-sync/src/main.rs`
- `aeon-sync/src/server.rs`

Tests first:

- same password decrypts an entry round trip
- wrong password fails authentication
- persisted file does not contain plaintext secret bytes
- metadata list excludes encrypted payload plaintext

Behavior:

- add password-derived vault key using PBKDF2 and per-vault salt
- add authenticated encryption for entry payloads
- add JSON-backed vault file
- add metadata list and create-entry endpoints
- avoid HTTP decrypt endpoint in this slice

Verification:

- `cargo test -p aeon-sync vault`

### 5. Query Foundation

Files:

- `aeon-sync/src/query.rs`
- `aeon-sync/src/server.rs`

Tests first:

- query filters captures by text
- query filters captures by kind
- query returns bounded results with stable summaries

Behavior:

- add deterministic `QueryRequest` and `QueryResponse`
- search existing capture records and event metadata
- add `/api/query`

Verification:

- `cargo test -p aeon-sync query`

### 6. Android SMS Preparation

Files:

- `aeon-android/app/src/main/java/flow/aeon/capture/VerificationCodeExtractor.kt`
- `aeon-android/app/src/test/java/flow/aeon/capture/VerificationCodeExtractorTest.kt`
- `aeon-android/app/src/main/java/flow/aeon/capture/AeonAgent.kt`

Tests first:

- Android extractor mirrors Rust SMS code patterns

Behavior:

- add pure Kotlin verification code extractor
- add agent method to post SMS bridge payload to `/api/bridge/sms`

Verification:

- `.\gradlew.bat :app:testDebugUnitTest`

### 7. Documentation

Files:

- `docs/EVENT_TIMELINE.md`
- `docs/CAPTURE.md`
- new `docs/AEON_FOUNDATIONS.md`

Behavior:

- document implemented endpoints
- document security limits
- document future integrations still requiring explicit permissions

## Final Verification

Run:

- `cargo fmt -- --check`
- `cargo test` in `aeon-capture`
- `cargo clippy --all-targets -- -D warnings` in `aeon-capture`
- `cargo test` in `aeon-sync`
- `cargo clippy --all-targets -- -D warnings` in `aeon-sync`
- Android unit tests if Gradle is available
- `git diff --check`
- `git status --short --branch`

## Commit Strategy

- commit design
- commit plan
- commit each implementation slice once its tests pass
- commit docs and final verification fixes
