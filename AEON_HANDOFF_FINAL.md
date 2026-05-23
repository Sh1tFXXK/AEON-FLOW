# AEON Flow Current Handoff

Date: 2026-05-23
Branch: `codex/feature-plateform`
Workspace: `E:\project\AEON-FLOW`

This file replaces the older handoff note, which was both stale and mojibake in
the current Windows checkout. Treat source code, tests, and the focused docs in
`docs/` as the authoritative project state.

## Current Direction

AEON Flow is a personal operating layer for capture, context, identity, and
cross-device continuity. The current implementation favors typed, local,
explicit capture sources over broad surveillance hooks:

- Screen context is captured from foreground window/process metadata and app
  bridges, not OCR.
- Text context is captured from committed OS/app surfaces such as Windows UI
  Automation, clipboard, files, Android share, SMS/email bridges, and explicit
  app APIs.
- Raw keyboard hooks and OS-level keylogging are not part of the accepted
  capture path.
- Screenshots can still be stored as image artifacts, but AEON does not derive
  timeline text by pixel OCR.

## Implemented Slices

- Desktop capture service, Web UI, LAN discovery, local relay, process panel,
  and capture stream.
- Typed OS activity capture through `aeon-capture::os_activity` for foreground
  window facts and UI Automation text commits.
- Event timeline/query foundation with deterministic "today what did I do"
  local summaries and optional structured planner integration.
- Server route split under `aeon-sync/src/server/`; route modules are kept below
  400 lines.
- Encrypted vault primitives, unlock sessions, TOTP support, password lookup,
  and browser extension credential-fill bridge.
- Multi-account Chrome profile planning and launch metadata.
- Direct IMAP email sync baseline using vault password credentials.
- Android share/photo capture baseline plus SMS bridge service and verification
  code extraction payloads.
- Browser extension bridge for URL context, verification code fill, and vault
  password fill.
- Local CLI account registry for `aeon account`, `aeon accounts`, and
  `aeon whoami` fallback behavior.
- Content-addressed CID sync can run as a bounded or long-running service with
  `aeon sync --serve <port|addr> [--sessions <n>]`.
- VM collaboration state now has a typed `SharedContext::merge_from` owner API
  plus a tested one-shot TCP context exchange for unique patches, messages, and
  sessions.
- VM daemon localhost TCP transport on non-Unix targets, heap editor behavior,
  snapshot/delta foundations, and documented VM limitations.
- Build outputs are ignored and removed from Git tracking; generated Android,
  Gradle, Rust, and output artifacts should remain local.

## Still Requires Real-World Validation

These areas are implemented as foundations but are not proven complete without
external devices or accounts:

- Android SMS bridge on a real device and OEM Android build.
- Android wireless discovery and share-to-desktop flow on the target LAN.
- Real Gmail/Outlook/IMAP provider sync with production credentials.
- Browser extension installation and autofill behavior on live login and
  verification-code pages.
- Multi-account browser workflows with real Chrome profiles and user data.
- Long-running CID sync across real devices and lossy networks.
- Public relay deployment, authentication policy, and cross-network device
  hardening.
- Persistent live VM multi-user editing beyond the current one-shot context
  exchange.

## Known Product Boundaries

- iOS SMS capture is not available through third-party app permissions.
- WeChat multi-account desktop operation remains constrained by the vendor app;
  VM or profile isolation is a separate integration path.
- The current AI query path is local/deterministic unless an explicit planner
  endpoint is configured.
- AEON VM remains a focused prototype with documented limits in
  `aeon-vm/KNOWN_LIMITATIONS.md`.

## Verification Commands

Use these commands before claiming a handoff-ready state:

```powershell
cd E:\project\AEON-FLOW\aeon-agent
cargo test

cd E:\project\AEON-FLOW\aeon-capture
cargo test os_activity

cd E:\project\AEON-FLOW\aeon-sync
cargo test email_sync

cd E:\project\AEON-FLOW\aeon-store
cargo test sync_listener_serves_multiple_announcements

cd E:\project\AEON-FLOW\aeon-vm
cargo test --test session
cargo test --test collab_transport
cargo test --bin aeon-console

cd E:\project\AEON-FLOW\aeon-android
.\gradlew.bat testDebugUnitTest assembleDebug

cd E:\project\AEON-FLOW
git diff --cached --check
git ls-files aeon-android/.gradle aeon-android/app/build aeon-android/build aeon-vm/target output tmp target
```

The final `git ls-files` command should print nothing when generated artifacts
are no longer tracked.
