# AEON End-to-End Smoke

This document describes manual smoke checks for a local AEON Flow stack. It is
not a replacement for crate tests; use it when validating desktop, Android,
Relay, and user-visible flows together.

## 1. Start The Desktop Stack

From the repository root:

```powershell
.\scripts\aeon.ps1
```

Expected output includes:

- `AEON LAN discovery listening on udp://0.0.0.0:8091`
- `AEON embedded Relay started`
- `AEON Flow capture service started`
- `Local: http://localhost:8080`
- `LAN: http://<desktop-lan-ip>:8080`
- `AEON Relay LAN: http://<desktop-lan-ip>:8090`

Open `http://localhost:8080`.

## 2. Capture Stream

Check these desktop flows:

- Copy text in Windows and confirm a clipboard entry appears.
- Add an image or screenshot artifact and confirm an image entry appears.
- Drag a file into the Web UI and confirm a CID is generated.
- Open item details for a text entry and confirm editing creates a new version.
- Confirm OS activity entries use window/process/app metadata, not OCR-derived
  screen text.

## 3. Process Panel

Open the process panel and verify:

- Known apps such as Claude Desktop, VS Code, Chrome, or Firefox show capture
  options when present.
- Unknown processes can still emit process metadata captures.
- AEON VM-managed processes show migration, snapshot, and pause/resume actions
  where available.

## 4. Android Wireless Discovery

Build and install the debug app:

```powershell
.\scripts\aeon.ps1 -Mode android
```

After install:

1. Open AEON Capture.
2. Tap `AUTO FIND AEON`.
3. Confirm the endpoint becomes the desktop LAN UI address, for example
   `http://192.168.x.x:8080`.
4. Confirm the desktop UI devices page shows the Android device online.
5. Share text, images, or files from any Android app to AEON.
6. Confirm the desktop capture stream receives the item.

USB fallback:

```powershell
.\scripts\aeon.ps1 -Mode android -UsbReverse
```

## 5. SMS Bridge

On a real Android device:

1. Open AEON Capture.
2. Open `Send / Settings`.
3. Tap `Start SMS bridge`.
4. Grant `READ_SMS` permission.
5. Receive a test verification SMS.
6. Confirm desktop receives a typed SMS bridge entry.
7. Confirm verification-code extraction surfaces the code.

Android emulator and desktop-only tests do not prove this flow because SMS
permission behavior is device and OS dependent.

## 6. Cross-Network Relay

On a reachable relay host:

```powershell
.\scripts\aeon.ps1 -Mode relay -RelayPort 8090 -Space home
```

On desktop:

```powershell
.\scripts\aeon.ps1 -Mode desktop -RelayUrl http://your-relay-host:8090 -Space home
```

On Android, set the endpoint to:

```text
http://your-relay-host:8090
```

Then share a payload and confirm the desktop imports it from Relay.

## 7. CID Sync Service

Run a bounded two-session listener:

```powershell
cd aeon-vm
cargo run --bin aeon -- sync --serve 127.0.0.1:7070 --sessions 2
```

From another terminal, announce two CIDs:

```powershell
cd aeon-vm
cargo run --bin aeon -- sync --announce <cid-1> --peer 127.0.0.1:7070
cargo run --bin aeon -- sync --announce <cid-2> --peer 127.0.0.1:7070
```

The listener should exit after two sessions and report requested/received/sent
counts. Use `--serve 7070` without `--sessions` for a long-running listener.

## 8. VM Collaboration Context Exchange

Run the automated one-shot TCP exchange check:

```powershell
cd aeon-vm
cargo test --test collab_transport
```

This validates the bounded context exchange path: one peer sends a
`SharedContext`, the receiver merges unique patches/messages/sessions through
`SharedContext::merge_from`, then returns its merged context. It does not prove
persistent live multi-user editing.

## 9. Optional Low-Level Agent Smoke

The early point-to-point agent can still be tested directly.

Terminal A:

```powershell
cd aeon-agent
$env:AEON_AGENT_LISTEN = "0.0.0.0:8787"
cargo run
```

Terminal B:

```powershell
cd aeon-agent
$env:AEON_AGENT_LISTEN = "0.0.0.0:8788"
$env:AEON_AGENT_PEER = "127.0.0.1:8787"
cargo run
```

## 10. Generated Artifacts

Before handoff, generated build output should not be tracked:

```powershell
git ls-files aeon-android/.gradle aeon-android/app/build aeon-android/build aeon-vm/target output tmp target
```

The command should print nothing.
