# AEON Flow

AEON Flow is a user-owned compute fabric for capture, context, identity,
execution, and cross-device continuity. It turns the user's authorized devices,
accounts, applications, files, browser state, runtime state, and task context
into one local-first operating layer.

The core abstraction is that a task belongs to AEON, not to one device. Any
device should be able to become an entry point into the same task surface, while
AEON handles state capture, capability routing, synchronization, recovery,
permissions, and audit history. AI sits beside this fabric as the reasoning
layer: AI can interpret intent and propose actions, but AEON owns the typed
state, device boundaries, credentials, and execution permissions.

AEON is not a generic file uploader, a pure AI memory product, or an
unauthorized network access tool. It only works across devices, accounts,
applications, and networks the user owns or explicitly authorizes.

## Product Direction

AEON is moving toward a personal compute fabric:

- **Universal task surface:** tasks are portable envelopes containing intent,
  context, inputs, outputs, permissions, execution log, and recovery points.
- **Device-independent access:** phones, desktops, browsers, VMs, and future
  agents are terminals into the same task fabric, not separate product silos.
- **State mirror:** authorized device state becomes typed, local, searchable,
  and resumable without relying on OCR, keylogging, or hidden mutation.
- **Capability abstraction:** low-level device, OS, app, account, and network
  differences are hidden behind explicit AEON capabilities.
- **Network-agnostic continuity:** LAN, relay, direct TCP, USB, portable sync
  packages, and future offline exchange are transport choices; the task model
  must not depend on always-on internet.
- **AI-adjacent execution:** AI can plan and request capabilities, but AEON
  enforces permissions, local ownership, provenance, and auditability.

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

Execution and synchronization remain explicitly bounded:

- Devices retain local ownership of their state and credentials.
- Background workers emit typed records and messages; they do not mutate
  render-visible state directly.
- Cross-device exchange is eventually consistent unless a specific runtime path
  documents stronger guarantees.
- Remote access must be authorized. AEON does not scan, exploit, or bypass
  third-party systems.

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
AEON Relay and connect each device to the same relay space. Relay is one
transport for the fabric, not the fabric itself; local LAN discovery and future
offline package exchange should remain first-class paths.

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
- `docs/`: capture, relay, event timeline, foundations, and smoke docs

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
