# AEON Flow Project Status

Date: 2026-08-13

This status note is based on a full repository scan of source files, manifests,
startup code, routes, and focused project documentation. It answers the practical
question: **where is the project now?**

## One-line Position

AEON Flow is currently an **early but broad local-first prototype**. The desktop
capture stack, local web UI, typed capture/event model, relay/LAN discovery,
Android ingress, browser extension bridge, credential/email/account foundations,
CID store, and AEON VM prototype all exist in code. The project is not yet a
hardened production product because several core promises still need real-world
validation with phones, browsers, email accounts, cross-network relays, and long
running multi-device use.

## Repository Shape

The repository is a multi-component prototype rather than a single application:

- `aeon-sync/`: primary desktop service and web UI. It starts capture workers,
  the HTTP API, WebSocket endpoint, embedded Relay, LAN discovery, relay pull
  and relay push loops.
- `aeon-capture/`: typed capture library. It owns capture entries, capture
  events, app/process/browser/terminal/VM capture helpers, clipboard, file,
  screenshot, and OS activity capture.
- `aeon-store/`: content-addressed data store, identity, account, chat/context,
  link/node objects, and CID sync messages.
- `aeon-vm/`: runnable-state VM prototype with assembler, snapshots, restore,
  migration, daemon, console, Forth prototype, JIT benchmarks, account registry,
  and one-shot collaboration context exchange.
- `aeon-android/`: Android capture app for share/file/photo/SMS ingress and LAN
  discovery.
- `aeon-browser-extension/`: Chromium extension for URL/title capture,
  verification-code fill, and vault-backed credential fill.
- `aeon-agent/`: older/early peer sync and collaboration agent prototype.
- `scripts/`: cross-platform launch wrappers and end-to-end smoke helper.
- `docs/`: product direction, capture policy, relay, event timeline, cross
  platform checklist, and smoke validation notes.

## What Is Implemented

### Desktop runtime

The desktop service creates `~/AEON` and `~/.aeon`, opens the local identity and
CID-backed capture store, starts clipboard, OS activity, screenshot, file-watch,
known-app capture, embedded Relay, LAN discovery, relay import/export loops, and
then serves the Axum web app on `0.0.0.0:<port>`.

### Web/API surface

The HTTP router includes routes for:

- status, device hello, WebSocket updates, events, and capture entries
- text/webpage/drop ingestion and raw CID download/editing
- files, uploads, downloads, save/delete, and history
- process/app/VM capture actions
- typed bridge payloads for SMS, email, and browser pages
- latest verification-code lookup for the local extension
- operation context, account profiles, credential vault, TOTP, unlock/fill
- email account import/direct sync
- deterministic and structured query execution

This means the project has a coherent desktop API surface, not just library
stubs.

### Capture model

Capture is explicitly **OS-event and app-surface first**:

- foreground window/process metadata and app bridges are preferred over OCR
- committed text can come from clipboard, files, UI Automation on Windows,
  Android share/SMS/email, browser bridge, or explicit APIs
- screenshots are stored as artifacts, not mined for timeline text
- raw keyboard hooks are outside the accepted pipeline

This is a clear product and privacy boundary in both code and docs.

### Cross-device path

The current cross-device path is:

1. desktop service exposes UI/API on port `8080` by default
2. embedded Relay exposes port `8090` by default
3. LAN discovery answers UDP `8091`
4. Android can auto-discover the desktop endpoint, then send shared text/files,
   photos, or SMS bridge payloads
5. Relay mode can carry payloads when direct LAN connectivity is unavailable

The code supports this path, but public relay hardening, auth policy, end-to-end
encryption, and lossy-network validation remain future work.

### Identity, vault, accounts, and email

The repository includes implemented foundations for:

- local identity creation/loading
- encrypted credential vault entries
- unlock sessions
- TOTP generation
- browser credential-fill lookup
- account profile registry and browser launch plans
- email account configs and cursor state
- direct Gmail/Outlook/IMAP sync paths that normalize into capture records

These are foundations: real provider credentials, OAuth app setup, browser
profile behavior, and live account validation still need explicit user/device
testing.

### Query and timeline

`POST /api/query` has a deterministic local fallback, including a local
"today what did I do" style summary. Optional planner configuration can turn a
natural language query into a structured local plan, but capture contents remain
local and invalid planner responses fall back to deterministic search.

### AEON VM

The VM is a separate prototype within the larger product. It supports:

- assembler and sample programs
- snapshots/restores and deltas
- migration sender/receiver path
- daemon and console tools
- Forth language prototype whose runtime state lives in VM state/VFS
- content-addressed sync command surface
- account registry commands
- one-shot collaboration context exchange
- JIT benchmark path for a small instruction subset

It is useful as a research/prototype slice for resumable computation, but it is
not the main desktop capture stack.

## What Is Tested In-repository

The repository has broad unit and integration-style coverage across the Rust
crates and Android unit tests. The test files cover store primitives, identity,
CID sync, capture event/store/engine logic, OS activity redaction, bridge
payloads, query planning/execution, vault behavior, email sync/IMAP parsing,
relay store behavior, agent state/collaboration, VM snapshots/sessions/editor,
and Android SMS/code extraction helpers.

The tests are strongest around deterministic local logic. They do not replace
manual validation on real Android devices, browser installations, real email
providers, and cross-network relay deployments.

## Current Gaps And Risks

- Android SMS capture and wireless discovery require real-device validation.
- Browser extension installation, code fill, and credential fill require live
  browser/page validation.
- Gmail, Outlook, and IMAP sync need real-account validation with explicit
  credentials and consent.
- Relay auth, public deployment hardening, and end-to-end encryption are not yet
  finished.
- Linux capture is more limited than Windows; Windows has richer UI Automation
  text commit support.
- Long-running CID sync and relay behavior need soak tests across real networks.
- AEON VM collaboration is a one-shot context exchange, not persistent live
  multi-user editing.
- The repository still contains multiple prototype slices, so product boundaries
  should continue to be documented whenever a feature graduates from foundation
  to validated user flow.

## Recommended Next Work

1. **Validation pass:** run the documented smoke flows on a Windows desktop,
   Android device, Chromium browser, and at least one real email account.
2. **Security pass:** define relay authentication, encryption posture, vault UX,
   and browser-extension trust boundaries before any public relay deployment.
3. **Product narrowing:** decide which flows are first-class for an MVP: desktop
   capture/search, Android share/SMS, browser context/fill, email sync, or VM
   continuity.
4. **Reliability pass:** add long-running service tests and recovery checks for
   relay pull/push, file watchers, event logs, and CID sync.
5. **Documentation pass:** keep `README.md`, `docs/CAPTURE.md`,
   `docs/RELAY.md`, and this status note synchronized with code after each
   implemented slice.

## Bottom Line

The project is past the idea-only phase: most subsystems have runnable code and
focused tests. It is best described as a **feature-rich proof of concept / alpha
foundation**. The next milestone is not adding more surfaces; it is proving the
existing surfaces end-to-end on real devices and accounts, then hardening the
smallest useful product path.
