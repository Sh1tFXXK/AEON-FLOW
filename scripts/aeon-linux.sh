#!/usr/bin/env bash
set -euo pipefail

MODE="all"
PORT=8080
RELAY_PORT=8090
DISCOVERY_PORT=8091
SPACE="home"
RELAY_URL=""
RELAY_DIR=""
NO_RELAY=0
RELEASE=0

usage() {
  cat <<USAGE
Usage: ./scripts/aeon-linux.sh [options]

Options:
  --mode <all|desktop|relay|stop>   Run mode (default: all)
  --port <port>                     UI port (default: 8080)
  --relay-port <port>               Relay port (default: 8090)
  --discovery-port <port>           UDP discovery port (default: 8091)
  --space <name>                    Relay space (default: home)
  --relay-url <url>                 External relay URL
  --relay-dir <path>                Relay data directory
  --no-relay                        Disable embedded relay for --mode all
  --release                         Run cargo with --release
  -h, --help                        Show help
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode) MODE="$2"; shift 2 ;;
    --port) PORT="$2"; shift 2 ;;
    --relay-port) RELAY_PORT="$2"; shift 2 ;;
    --discovery-port) DISCOVERY_PORT="$2"; shift 2 ;;
    --space) SPACE="$2"; shift 2 ;;
    --relay-url) RELAY_URL="$2"; shift 2 ;;
    --relay-dir) RELAY_DIR="$2"; shift 2 ;;
    --no-relay) NO_RELAY=1; shift ;;
    --release) RELEASE=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $1"; usage; exit 2 ;;
  esac
done

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR/aeon-sync"

cargo_cmd=(cargo run)
if [[ "$RELEASE" -eq 1 ]]; then
  cargo_cmd+=(--release)
fi
cargo_cmd+=(--)

run_sync() {
  "${cargo_cmd[@]}" "$@"
}

case "$MODE" in
  all)
    args=(start --port "$PORT" --relay-port "$RELAY_PORT" --relay-space "$SPACE" --discovery-port "$DISCOVERY_PORT")
    if [[ "$NO_RELAY" -eq 1 ]]; then
      args+=(--no-relay)
    else
      args+=(--with-relay)
    fi
    [[ -n "$RELAY_URL" ]] && args+=(--relay-url "$RELAY_URL")
    [[ -n "$RELAY_DIR" ]] && args+=(--relay-dir "$RELAY_DIR")
    run_sync "${args[@]}"
    ;;
  desktop)
    args=(start --port "$PORT" --no-relay --relay-space "$SPACE" --discovery-port "$DISCOVERY_PORT")
    [[ -n "$RELAY_URL" ]] && args+=(--relay-url "$RELAY_URL")
    run_sync "${args[@]}"
    ;;
  relay)
    args=(relay --port "$RELAY_PORT" --space "$SPACE")
    [[ -n "$RELAY_DIR" ]] && args+=(--dir "$RELAY_DIR")
    run_sync "${args[@]}"
    ;;
  stop)
    pkill -f "target/.*/aeon-sync|aeon-sync" || true
    echo "Stopped aeon-sync processes (if running)."
    ;;
  *)
    echo "Invalid mode: $MODE"
    usage
    exit 2
    ;;
esac
