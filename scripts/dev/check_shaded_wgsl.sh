#!/usr/bin/env bash
# Checks an app's shaded-surface WGSL the way the renderer builds it: the
# framework's prelude (mesh_common.wgsl, mesh_shaded.wgsl) followed by the
# given files, in order. naga validates the whole module and translates its
# fragment entry points to GLSL ES 3.0 (WebGL) and Metal, so a shader that one
# backend refuses fails here instead of drawing lit in the app.
#
# Usage: scripts/dev/check_shaded_wgsl.sh <file.wgsl>...
set -euo pipefail

if [[ $# -eq 0 ]]; then
  echo "usage: $0 <file.wgsl>..." >&2
  exit 2
fi
if ! command -v naga >/dev/null; then
  echo "naga is not installed: cargo install naga-cli" >&2
  exit 2
fi

root="$(cd "$(dirname "$0")/../.." && pwd)"
shaders="$root/crates/cranpose-ui-graphics/shaders"
work="$(mktemp -d)"
trap 'rm -r "$work"' EXIT

module="$work/shaded.wgsl"
cat "$shaders/mesh_common.wgsl" "$shaders/mesh_shaded.wgsl" "$@" >"$module"

naga "$module"
for entry in mesh_shaded_fs mesh_shaded_shadowed_fs mesh_shaded_portal_fs; do
  naga --entry-point "$entry" --profile es300 "$module" "$work/$entry.frag"
done
naga "$module" "$work/shaded.metal"
echo "ok: $*"
