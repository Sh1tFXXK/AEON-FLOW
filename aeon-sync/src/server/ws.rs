use super::*;

use super::shared::*;

pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(|socket| handle_ws(socket, state))
}

async fn handle_ws(mut socket: WebSocket, state: AppState) {
    let mut file_rx = state.file_events.subscribe();
    let mut capture_rx = state.capture_engine.subscribe();

    loop {
        let payload = tokio::select! {
            Ok(event_name) = file_rx.recv() => serde_json::json!({
                "type": "refresh",
                "event": event_name,
                "at": now_ms(),
            }),
            Ok(entry) = capture_rx.recv() => {
                let record = capture_record_from_entry(&entry);
                serde_json::json!({
                    "type": "capture",
                    "entry": capture_payload(record),
                    "at": now_ms(),
                })
            },
            else => break,
        };

        if socket
            .send(Message::Text(payload.to_string()))
            .await
            .is_err()
        {
            break;
        }
    }
}
