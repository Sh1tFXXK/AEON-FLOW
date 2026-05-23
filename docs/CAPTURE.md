# AEON Capture

AEON Capture turns local activity into typed `CaptureEntry` records. The current direction is OS-event first: AEON should prefer operating-system and application APIs over image recognition or broad input hooks.

## Capture Policy

- Screen context is captured from OS window/process state, not OCR.
- Text is captured only from committed content surfaces: clipboard, files, app bridges, browser bridges, SMS/email bridges, or explicit user share actions.
- Raw keyboard hooks are not a capture source.
- Screenshots may be stored as image artifacts, but AEON does not derive `ScreenText` from pixels.
- Sensitive or unknown text commits are redacted before storage.
- Background workers create typed `CaptureEntry` values and pass them through `CaptureEngine`; they do not mutate UI-visible state directly.

## Active Capture Surfaces

- Clipboard text on Windows.
- Foreground window focus through OS APIs as `CaptureKind::OsActivity`.
- Focused control committed text through Windows UI Automation as `OsActivity::TextCommit`.
- Browser live tab URL/title facts through the AEON Browser Bridge extension.
- Screenshot directories as image artifacts.
- `~/AEON` file monitoring.
- Known app bridges for Claude Desktop, VS Code, browsers, terminals, processes, and AEON VM snapshots.
- Android share/photo ingress and typed bridge payloads for SMS/email.
- Email sync batch import for provider workers and direct Gmail/Outlook/IMAP sync.
- Relay import/export for cross-device capture sync.

## OS Activity

`aeon-capture::os_activity` owns the OS-level activity contract.

- `OsActivity::WindowFocus` records foreground window title, process id, process name, and bounds when available.
- `OsActivity::TextCommit` records committed text from trusted OS/application APIs. It is not a keystroke log.
- `InputSensitivity::Sensitive` and `InputSensitivity::Unknown` remove the text payload before storage.
- `CaptureSource::OperatingSystem` stores a typed `OsCaptureProvider` enum, not a string provider name.

On Windows the desktop service starts foreground-window and UI Automation text-commit monitors. This provides the "what am I looking at and editing?" timeline layer without OCR or raw keyboard hooks.

## HTTP API

- `GET /api/status`: local identity, device, and connection addresses.
- `GET /api/entries`: recent capture stream.
- `GET /api/entry/:cid`: metadata and UTF-8 preview.
- `GET /api/entry/:cid/raw`: raw CID bytes.
- `POST /api/entry/:cid/edit`: edit text-like captures and create a new version.
- `POST /api/capture/text`: JSON body `{ "text": "...", "title": "..." }`.
- `POST /api/capture/drop`: multipart field `file`, used by web drag/drop and Android file share.
- `POST /api/capture/apps`: capture all known running app bridges.
- `GET /api/processes`: process panel listing.
- `POST /api/capture-process`: process metadata, screenshot artifact, VM snapshot, or migration action.
- `POST /api/query`: deterministic capture search and today-activity summary.
- `POST /api/query/structured`: execute a typed query plan produced by a local
  or hosted LLM planner.
- `GET /api/email/accounts`: list configured email sync accounts.
- `POST /api/email/accounts`: upsert a typed IMAP/Gmail/Outlook-style email sync account.
- `POST /api/email/accounts/:id/import`: import already-fetched email messages,
  deduplicate by message id, update cursor state, and capture new messages.
- `POST /api/email/accounts/:id/sync`: loopback-only Gmail/Outlook/IMAP sync
  using an unlocked vault credential referenced by the account config.

## Email Sync

Email sync has two ingestion modes:

- Provider workers may still call `POST /api/email/accounts/:id/import` with
  already-fetched typed messages.
- AEON can directly call Gmail and Microsoft Graph for accounts configured as
  `GmailApi` or `OutlookApi` through `POST /api/email/accounts/:id/sync`.
- AEON can directly call IMAP servers for accounts configured as `Imap`.

Direct sync requires:

- local loopback caller
- an unexpired vault unlock `session_id`
- `credential_ref` pointing at an `OAuthToken` vault entry for Gmail/Outlook, or
  a `Password` vault entry for IMAP
- for refreshable tokens, `token_url` and `client_id` in that vault entry;
  `client_secret` is included when present

Gmail sync lists message ids first, then fetches metadata headers for each
message. Outlook sync calls Microsoft Graph messages with a narrow `$select`
field set. If the OAuth access token is expired or near expiry, AEON refreshes
it through the provider token endpoint and persists the rotated encrypted token
before fetching mail.

IMAP sync opens a direct TCP or TLS connection, logs in with the vault-backed
password credential, selects the configured mailbox, runs `UID SEARCH ALL`, and
fetches recent messages with `UID FETCH ... BODY.PEEK[]`. Parsed headers and
body preview are normalized to the same import path as API provider messages.
The current connector is verified with local protocol tests; provider-specific
IMAP quirks and real-account validation remain explicit follow-up work.

## Query Planner

`POST /api/query` always has a deterministic local fallback. If these
environment variables are set, AEON first asks the configured planner to return
a typed `StructuredQueryPlan`, then executes that plan locally against the
capture index:

- `AEON_QUERY_PLANNER_URL`: planner HTTP endpoint.
- `AEON_QUERY_PLANNER_MODEL`: model name sent to the endpoint.
- `AEON_QUERY_PLANNER_PROVIDER`: `openai-compatible` or `ollama-chat`
  (defaults to `openai-compatible`).
- `AEON_QUERY_PLANNER_API_KEY`: optional bearer token.

The planner receives only the question and current Unix millisecond timestamp;
it does not receive capture contents. Invalid, empty, or unreachable planner
responses fall back to deterministic local query behavior.

## Android

`aeon-android/` is the mobile ingress:

- `ShareReceiverActivity` receives shared text/files/images.
- Text share calls `/api/capture/text`.
- File/image share calls `/api/capture/drop`.
- `PhotoWatcherService` monitors `MediaStore.Images` and captures new photos.
- `SmsWatcherService` observes `content://sms` after `READ_SMS` is granted and
  sends typed SMS bridge payloads to `/api/bridge/sms`.
- `MainActivity` discovers the desktop endpoint through AEON LAN discovery.
- `MainActivity` exposes `Start SMS bridge` in the Send / Settings tab.

Build and install:

```powershell
.\scripts\aeon.ps1 -Mode android
```

## Browser Extension

`aeon-browser-extension/` is a local Chrome/Chromium extension:

- `background.js` reports completed and activated web tabs to
  `POST /api/bridge/browser-page`.
- The bridge stores the URL/title/tab metadata as a normal
  `CaptureKind::Webpage` record with `bridge.kind=browser_page`.
- `content.js` supports an explicit `aeon_fill_verification_code` message for
  controlled verification-code fill actions.
- The background worker polls loopback-only
  `GET /api/bridge/verification-code/latest` on a timer and when the active tab
  changes. If the code is still inside its five-minute expiry window, it asks
  the content script to fill the current page's likely verification-code input.
- The popup unlocks the AEON credential vault for a short in-memory session and
  can request credential fill for the active tab URL.
- Credential fill uses `POST /api/vault/fill`, which only returns a matching
  `Password` entry when `auto_fill=true`, the domain matches, and the request is
  from loopback with an unexpired session.

Load it from `chrome://extensions` with Developer mode and "Load unpacked".
It does not read cookies, export sessions, or perform browser-native password
import/export.

## Current Boundaries

- Browser capture supports recent history, app bridges, and the local unpacked
  browser extension for live URL/title facts. Full DOM capture and native
  messaging are still separate work.
- Ordinary process capture stores metadata and optional window image artifacts. Complete runtime migration only applies to AEON VM managed processes.
- No OCR pipeline is part of capture.
- No raw keyboard capture is part of capture.
- Android SMS capture requires user-granted `READ_SMS` and must be validated on
  a real device because Android SMS access is platform-policy constrained.
- Verification-code auto-fill depends on a loaded Chromium extension and a page
  with a detectable text/tel/numeric/code input. AEON exposes only the latest
  derived code, address, timestamp, and expiry metadata to the local extension;
  non-loopback clients are rejected.
- Credential auto-fill depends on an explicit popup unlock action and an
  unexpired vault session. AEON does not submit login forms.
- Email sync currently owns account config, cursor state, deduplication, and
  capture import. Gmail, Outlook, and baseline IMAP direct sync are implemented;
  provider-specific quirks and real-account validation still require credentials
  and explicit consent.
