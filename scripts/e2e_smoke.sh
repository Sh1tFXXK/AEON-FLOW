#!/usr/bin/env bash
set -euo pipefail
echo "[1] identity show"
cd aeon-store && cargo run --bin aeon-data -- identity show || true
cd ..
echo "[2] api status"
curl -sf http://127.0.0.1:8080/api/status || true
