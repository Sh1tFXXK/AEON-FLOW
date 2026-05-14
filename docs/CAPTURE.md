# AEON Capture

AEON Capture turns "what I am doing" into content-addressed entries. Files, clipboard text, screenshots, app state, browser pages, and Android shares all enter the same stream.

## Run

```bash
cd aeon-sync
cargo run
```

Open `http://localhost:8080`.

The service starts these capture sources:

- Windows clipboard text monitor.
- Screenshot directory monitor.
- `~/AEON` file monitor.
- Claude Desktop and VS Code app watchers when those apps are running.
- Web UI drag-and-drop capture.

## HTTP API

- `GET /api/entries`: recent capture stream.
- `GET /api/entry/:cid`: metadata and UTF-8 preview.
- `GET /api/entry/:cid/raw`: original bytes from CID storage.
- `POST /api/capture/text`: JSON body `{ "text": "...", "title": "..." }`.
- `POST /api/capture/drop`: multipart field `file`, used by drag-and-drop and Android file sharing.
- `POST /api/capture/apps`: capture currently running registered apps.
- `POST /api/capture/process/:pid`: capture process metadata for a PID.

## Android

`aeon-android/` is a minimal Android app:

- `ShareReceiverActivity` registers as an Android share target for any MIME type.
- Text shares call `/api/capture/text`.
- File/image shares call `/api/capture/drop`.
- `PhotoWatcherService` observes `MediaStore.Images` and captures new photos.
- `MainActivity` stores the AEON server endpoint, such as `http://192.168.1.8:8080`.

Build it with Android Studio or a local Android Gradle setup.

## Current Boundaries

- Browser capture uses the latest history entry for Chrome/Firefox. True active-tab capture needs a browser extension or native messaging host.
- Process capture stores process metadata. Full memory snapshots for arbitrary processes still require privileged native support.
- Windows tray OLE drag/drop is not implemented; browser drag/drop and folder watch provide the working capture path.
