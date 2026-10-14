#!/usr/bin/env bash
# Prints the path of the emsdk_env.sh that sets up Emscripten $1, after
# installing that version under ~/.cache/cranpose-emsdk on first use. One
# lock serialises installs between jobs on a host.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
version="${1:?usage: emsdk_env.sh <emscripten version>}"
root="${CRANPOSE_EMSDK:-$HOME/.cache/cranpose-emsdk}"
mkdir -p "$(dirname "$root")"
exec 9>"$root.lock"
python3 "$SCRIPT_DIR/host_flock.py" -x 9
if [ ! -d "$root/.git" ]; then
  git clone --depth 1 https://github.com/emscripten-core/emsdk.git "$root" >&2
fi
if [ ! -f "$root/.installed-$version" ]; then
  (cd "$root" && ./emsdk install "$version" >&2 && ./emsdk activate "$version" >&2)
  touch "$root/.installed-$version"
fi
echo "$root/emsdk_env.sh"
