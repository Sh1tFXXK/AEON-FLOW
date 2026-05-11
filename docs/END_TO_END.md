# AEON End-to-End Smoke

## 1. Identity
- `cd aeon-store && cargo run --bin aeon-data -- identity new`
- `cargo run --bin aeon-data -- identity show`

## 2. Agent A/B sync
- Terminal A: `cd aeon-agent && AEON_AGENT_LISTEN=0.0.0.0:8787 cargo run`
- Terminal B: `cd aeon-agent && AEON_AGENT_LISTEN=0.0.0.0:8788 AEON_AGENT_PEER=127.0.0.1:8787 cargo run`

## 3. HTTP UI
- `cd aeon-sync && cargo run`
- open `http://localhost:8080`
- upload file and verify history in right pane.

## 4. Android Termux
- `pkg install rust git`
- `git clone <repo> && cd AEON-FLOW/aeon-agent`
- `AEON_AGENT_LISTEN=0.0.0.0:8787 cargo run`


## 5. Reconnect auto-peering
- use `AEON_AGENT_PEERS="127.0.0.1:8787,127.0.0.1:8788"`
- agent retries missing peers every 5 seconds.
