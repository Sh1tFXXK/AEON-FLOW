#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if command -v cygpath >/dev/null 2>&1; then
  PS_SCRIPT="$(cygpath -w "${SCRIPT_DIR}/aeon.ps1")"
  PS_EXE="$(cygpath -u "${WINDIR:-C:\\Windows}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe")"
else
  PS_SCRIPT="${SCRIPT_DIR}/aeon.ps1"
  PS_EXE="powershell.exe"
fi

"${PS_EXE}" -NoProfile -ExecutionPolicy Bypass -File "${PS_SCRIPT}" "$@"
