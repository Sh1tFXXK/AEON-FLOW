# AEON Flow

AEON Flow is a personal operating layer for capture, context, identity, and
cross-device continuity. It is not a generic file uploader. It turns current
work into local, addressable, searchable data that can be synced and resumed
across devices.

## Project Status

As of 2026-08-13, AEON Flow is a feature-rich local-first proof of concept / alpha foundation: many subsystems are runnable and tested locally, but real-device, real-account, and public-relay hardening work remains before it should be treated as production-ready. See [docs/PROJECT_STATUS.md](docs/PROJECT_STATUS.md) for the repository-wide status assessment.

## Current Direction

Capture is OS-event first:

- Screen context comes from foreground window/process metadata and app/browser
  bridges, not OCR.
- Text context comes from committed OS/app surfaces: Windows UI Automation,
  clipboard, files, Android share, SMS/email bridges, and explicit app APIs.
- Raw keyboard hooks and OS-level keylogging are not part of the capture
  pipeline.
- Screenshots can be stored as image artifacts, but AEON does not derive
  timeline text from pixels.

## Quick Start

From the repository root:

```powershell
.\scripts\aeon.ps1
```

Other wrappers:

```cmd
scripts\aeon.cmd
```

```bash
# Linux/macOS (native shell launcher)
./scripts/aeon-linux.sh

# Windows-compatible wrapper (calls powershell.exe)
./scripts/aeon.sh
```

Default local endpoints:

- Web UI: `http://localhost:8080`
- Embedded Relay: `http://localhost:8090`
- LAN Discovery: UDP `8091`

The Windows script also prepares inbound firewall rules for the UI, Relay, and
LAN discovery ports when it has permission to do so. To stop local AEON
processes started by the script:

```powershell
.\scripts\aeon.ps1 -Mode stop
```


## Linux Status (Important)

On Linux, AEON service startup works, but some automatic desktop capture sources are currently Windows-only:

- Clipboard background monitor is gated to Windows builds.
- Foreground window / text commit OS activity monitors are gated to Windows builds.
- Per-process window screenshot capture is Windows-only.

Linux users can still capture content through:

- Web API and UI actions (`/api/capture/text`, drag/drop to `/api/capture/drop`).
- File and screenshot directory watchers when files are created in watched paths.
- Relay imports and Android share/SMS/email bridge flows.

## Query Planner

`POST /api/query` works without a model through deterministic local filters and
the built-in "today what did I do" summary. To let a local or hosted planner
translate natural language into typed filters, set:

- `AEON_QUERY_PLANNER_URL`
- `AEON_QUERY_PLANNER_MODEL`
- `AEON_QUERY_PLANNER_PROVIDER=openai-compatible|ollama-chat`
- `AEON_QUERY_PLANNER_API_KEY` (optional)

The planner sees only the question and current Unix millisecond timestamp.
Capture contents remain local, and invalid planner responses fall back to the
local query path.

## Device Flow

On the same LAN, Android uses AEON LAN Discovery:

1. Start the desktop stack with `.\scripts\aeon.ps1`.
2. Build/install Android with `.\scripts\aeon.ps1 -Mode android`.
3. Open AEON Capture on Android.
4. Tap `AUTO FIND AEON`.
5. Confirm the endpoint is the desktop LAN UI address, for example
   `http://192.168.x.x:8080`.
6. Share text, images, or files from any Android app to AEON.
7. The desktop capture stream should show the item.

USB reverse is a development fallback:

```powershell
.\scripts\aeon.ps1 -Mode android -UsbReverse
```

## Relay Mode

When devices cannot directly reach the desktop LAN, run a public or private
AEON Relay and connect each device to the same relay space.

Relay only:

```powershell
.\scripts\aeon.ps1 -Mode relay -RelayPort 8090 -Space home
```

Desktop connected to a relay:

```powershell
.\scripts\aeon.ps1 -Mode desktop -RelayUrl http://your-relay-host:8090 -Space home
```

See [docs/RELAY.md](docs/RELAY.md).

## Android SMS Bridge

In AEON Capture, open `Send / Settings`, then tap `Start SMS bridge`.

- Android requests `READ_SMS`.
- Without permission, the service stops.
- Granted SMS rows are converted into typed bridge payloads and posted to
  `/api/bridge/sms`.
- Runtime behavior must be verified on a real Android device because SMS access
  depends on Android version, OEM policy, and user permission.

## CID Sync Service

`aeon-store` content sync can run as a one-shot listener or as a long-running
service.

Long-running service:

```powershell
cd aeon-vm
cargo run --bin aeon -- sync --serve 7070
```

Bounded smoke test:

```powershell
cargo run --bin aeon -- sync --serve 127.0.0.1:7070 --sessions 2
cargo run --bin aeon -- sync --announce <cid> --peer 127.0.0.1:7070
```

## VM Collaboration Context

`aeon-vm` collaboration state is merged through the typed
`SharedContext::merge_from` API. The `collab_transport` module adds a tested
one-shot TCP exchange so two peers can swap context state and import unique
patches, messages, and sessions. This is intentionally small; persistent live
multi-user editing remains outside the current boundary.

## AEON VM Quick Start

```powershell
cd aeon-vm
cargo build --release
cargo run --bin aeon-asm -- programs\fibonacci.asm -o fibonacci.aeon
cargo run --bin aeon-run -- fibonacci.aeon
```

More VM details are in [aeon-vm/README.md](aeon-vm/README.md).

## Repository Layout

- `aeon-sync/`: desktop Web UI, capture API, process panel, embedded Relay, LAN
  discovery
- `aeon-capture/`: capture entries, capture engine, clipboard/screenshot/file
  and OS activity capture
- `aeon-android/`: Android share entrypoint, photo watcher, SMS bridge, discovery
- `aeon-store/`: CID store, identity, collaboration objects, content sync
- `aeon-vm/`: snapshot, restore, migration, daemon, and CLI prototype
- `aeon-agent/`: early point-to-point sync agent
- `aeon-browser-extension/`: browser URL/code/password fill bridge
- `docs/`: capture, relay, event timeline, architecture, and smoke docs

## Development Checks

Run focused tests near the crate you changed. Useful baselines:

```powershell
cd aeon-capture
cargo test os_activity
```

```powershell
cd aeon-sync
cargo test email_sync
```

```powershell
cd aeon-store
cargo test
```

```powershell
cd aeon-vm
cargo test --bin aeon
cargo test --test session
cargo test --test collab_transport
```

For Android:

```powershell
cd aeon-android
.\gradlew.bat testDebugUnitTest assembleDebug
```
