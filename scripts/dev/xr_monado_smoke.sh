#!/usr/bin/env bash
# Runs apps/xr-demo against Monado's simulated headset and its null compositor, and prints the
# frames it ran. Needs `monado-service` on PATH and the OpenXR loader (`libopenxr_loader.so.1`).
# Usage: scripts/dev/xr_monado_smoke.sh [seconds]
set -euo pipefail

seconds="${1:-8}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

if ! command -v monado-service >/dev/null; then
  echo "xr-smoke: monado-service is not on PATH; install Monado first" >&2
  exit 2
fi

workdir="$(mktemp -d)"
service=""
cleanup() {
  if [[ -n "$service" ]]; then
    kill "$service" 2>/dev/null || true
  fi
}
trap cleanup EXIT

# The names are Monado's own: SIMULATED_ENABLE turns on the simulated headset builder,
# XRT_COMPOSITOR_NULL picks the compositor that shows nothing, and its FPS option paces it.
export SIMULATED_ENABLE=true
export XRT_COMPOSITOR_NULL=true
export XRT_COMPOSITOR_NULL_FPS=60
runtime_json=/usr/share/openxr/1/openxr_monado.json
if [[ -z "${XR_RUNTIME_JSON:-}" && -f "$runtime_json" ]]; then
  export XR_RUNTIME_JSON="$runtime_json"
fi

monado-service >"$workdir/monado.log" 2>&1 &
service=$!
socket="${XDG_RUNTIME_DIR:-/tmp}/monado_comp_ipc"
for _ in $(seq 1 50); do
  [[ -S "$socket" ]] && break
  kill -0 "$service" 2>/dev/null || { cat "$workdir/monado.log" >&2; exit 1; }
  sleep 0.2
done

cd "$root"
cargo build -q -p xr-demo
set +e
RUST_LOG=info,cranpose_xr=debug timeout "$seconds" "${CARGO_TARGET_DIR:-target}/debug/xr-demo" >"$workdir/demo.log" 2>&1
set -e
cat "$workdir/demo.log"

frames="$(grep -o 'cranpose xr: [0-9]* frames' "$workdir/demo.log" | awk '{sum += $3} END {print sum + 0}')"
echo "xr-smoke: $frames frames in about $seconds s"
if [[ "$frames" -eq 0 ]]; then
  echo "xr-smoke: the demo drew no frame" >&2
  exit 1
fi
