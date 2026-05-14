# AEON End-to-End Smoke

## 1. Identity
- `cd aeon-store && cargo run --bin aeon-data -- identity new`
- `cargo run --bin aeon-data -- identity show`

## 2. Agent A/B sync
- Terminal A: `cd aeon-agent && AEON_AGENT_LISTEN=0.0.0.0:8787 cargo run`
- Terminal B: `cd aeon-agent && AEON_AGENT_LISTEN=0.0.0.0:8788 AEON_AGENT_PEER=127.0.0.1:8787 cargo run`

## 3. Capture UI
- `cd aeon-sync && cargo run`
- open `http://localhost:8080`
- copy text on Windows and verify it appears in the capture stream.
- drag a file into the browser and verify it appears as a capture entry.
- use `POST /api/capture/text` for API smoke tests.

## 4. Android Termux
- `pkg install rust git`
- `git clone <repo> && cd AEON-FLOW/aeon-agent`
- `AEON_DEVICE_NAME=android-phone AEON_AGENT_LISTEN=0.0.0.0:8787 cargo run`

## 5. Android node mode (Step 5)
- Default Android watch roots: `/sdcard/DCIM/Camera`, `/sdcard/Download`, `/sdcard/Documents`, `/sdcard/Pictures`, and `~/AEON`.
- Override watch roots explicitly with:
  - `AEON_SYNC_DIRS="/sdcard/DCIM/Camera,/sdcard/Download,/sdcard/Documents"`
- Example with bootstrap peer:
  - `AEON_DEVICE_NAME=android-phone AEON_AGENT_LISTEN=0.0.0.0:8787 AEON_AGENT_PEER=192.168.1.8:8787 cargo run`


## 6. Reconnect auto-peering
- use `AEON_AGENT_PEERS="127.0.0.1:8787,127.0.0.1:8788"`
- agent retries missing peers every 5 seconds.

## 8. Android share app
- Open `aeon-android/` in Android Studio.
- Set the server endpoint to the LAN URL shown by `aeon-sync`, for example `http://192.168.1.8:8080`.
- Share text or images from any Android app to `AEON`.
- Verify the entry appears in the capture stream on Windows.


## 7. Tombstone semantics
- Remote delete applies only when local file is not newer than tombstone timestamp.
- On startup, persisted tombstones are replayed against sync root for cold-start consistency.
