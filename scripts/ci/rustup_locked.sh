#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
real_home="$(eval echo "~$(id -un)")"
export RUSTUP_HOME="${RUSTUP_HOME:-$real_home/.rustup}"
mkdir -p "$RUSTUP_HOME"
exec 9>"$RUSTUP_HOME/cranpose-install.lock"
python3 "$SCRIPT_DIR/host_flock.py" -x 9
exec rustup "$@"
