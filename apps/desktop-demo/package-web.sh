#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUTPUT_DIR="${1:?Usage: package-web.sh OUTPUT_DIR [PACKAGE_DIR] [INDEX_HTML]}"
PACKAGE_DIR="${2:-$SCRIPT_DIR/pkg}"
INDEX_HTML="${3:-$SCRIPT_DIR/index.html}"
MODULE_FILE="$PACKAGE_DIR/desktop_app.js"
WASM_FILE="$PACKAGE_DIR/desktop_app_bg.wasm"

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$@" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$@" | awk '{print $1}'
    else
        echo "A SHA-256 tool (sha256sum or shasum) is required" >&2
        exit 1
    fi
}

for required_file in "$INDEX_HTML" "$MODULE_FILE" "$WASM_FILE"; do
    if [ ! -f "$required_file" ]; then
        echo "Required web release file is missing: $required_file" >&2
        exit 1
    fi
done

if [ -e "$OUTPUT_DIR" ]; then
    echo "Web release output already exists: $OUTPUT_DIR" >&2
    exit 1
fi

module_hash="$(sha256 "$MODULE_FILE")"
wasm_hash="$(sha256 "$WASM_FILE")"
bundle_hash="$(printf '%s%s' "$module_hash" "$wasm_hash" | sha256)"
bundle_relative="assets/$bundle_hash"
bundle_dir="$OUTPUT_DIR/$bundle_relative"

mkdir -p "$bundle_dir"
cp "$INDEX_HTML" "$OUTPUT_DIR/index.html"
cp "$MODULE_FILE" "$bundle_dir/desktop_app.js"
cp "$WASM_FILE" "$bundle_dir/desktop_app_bg.wasm"
touch "$OUTPUT_DIR/.nojekyll"

printf '{\n  "version": 1,\n  "module": "%s/desktop_app.js",\n  "wasm": "%s/desktop_app_bg.wasm"\n}\n' \
    "$bundle_relative" \
    "$bundle_relative" \
    > "$OUTPUT_DIR/asset-manifest.json"

echo "Packaged content-addressed web bundle: $bundle_relative"
