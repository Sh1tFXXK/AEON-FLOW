# AEON Relay

AEON Relay is the cross-network capture channel for AEON Flow. It lets remote
devices submit capture payloads to a relay space, then lets the desktop pull
those payloads into the local capture stream.

On the same LAN, prefer AEON LAN Discovery. Use Relay when the phone or remote
device cannot directly reach the desktop.

## Model

```text
Android or remote device
        POST /api/capture/text or /api/capture/drop
        |
        v
AEON Relay
        stores items by space and cursor
        |
        v
Desktop AEON
        GET /api/relay/pull
        imports into local capture stream
```

## Local Full Stack

Start desktop UI, embedded Relay, and LAN discovery:

```powershell
.\scripts\aeon.ps1
```

Default ports:

- UI: `http://localhost:8080`
- Relay: `http://localhost:8090`
- LAN Discovery: UDP `8091`

The Windows startup script attempts to create inbound firewall rules:

- `AEON Flow UI TCP 8080`
- `AEON Flow Relay TCP 8090`
- `AEON Flow Discovery UDP 8091`

Check them with:

```powershell
Get-NetFirewallRule -DisplayName 'AEON Flow*'
```

## Android LAN Connection

Recommended local flow:

1. Start desktop with `.\scripts\aeon.ps1`.
2. Install and open AEON Capture on Android.
3. Tap `AUTO FIND AEON`.
4. Confirm the endpoint is `http://<desktop-lan-ip>:8080`.
5. Share text, files, or images from Android apps to AEON.
6. Confirm the desktop capture stream receives the item.

USB reverse is only a development fallback:

```powershell
.\scripts\aeon.ps1 -Mode android -UsbReverse
```

## Public Relay Node

Start only the Relay on a public or private reachable host:

```powershell
.\scripts\aeon.ps1 -Mode relay -RelayPort 8090 -Space home
```

Connect desktop to that relay:

```powershell
.\scripts\aeon.ps1 -Mode desktop -RelayUrl http://your-relay-host:8090 -Space home
```

Use the same relay URL from Android:

```text
http://your-relay-host:8090
```

## API

- `POST /api/devices/hello`
- `POST /api/capture/text`
- `POST /api/capture/drop`
- `POST /api/relay/push`
- `GET /api/relay/pull?space=home&after=<cursor>`
- `GET /api/relay/status`

## Implemented

- Android/remote text capture enters Relay.
- Android/remote files and images enter Relay.
- Desktop polling imports Relay payloads into the local capture stream.
- Relay spaces, cursors, and device identity headers exist.
- `aeon-sync` embeds Relay so one process can start the desktop stack.
- LAN Discovery answers UDP `8091` so same-LAN devices do not need USB or
  third-party tunnels.

## Not Yet Hardened

- Relay auth token policy.
- End-to-end encryption for public relay deployments.
- WebSocket or long-polling replacement for desktop polling.
- Relay-carried AEON VM snapshot handoff.
