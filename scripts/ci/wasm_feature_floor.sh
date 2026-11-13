#!/usr/bin/env bash
# Fails when a WebAssembly module needs a feature past Safari 15.
#
# Usage: wasm_feature_floor.sh MODULE
#
# The allowed features are the ones rustc emits for wasm32-unknown-unknown.
# Safari 15 and every later browser has all of them. WebKit rejects the whole
# module when it uses any other feature, such as the typed function references
# of Safari 18. The page then shows only its error screen on such an iPhone, in
# Safari and in every other iOS browser. wasm-tools checks every type and
# instruction against the list and ignores the module's own target_features
# section.
set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "usage: $0 MODULE" >&2
    exit 2
fi
module=$1
version=1.263.0
features=wasm1,bulk-memory,mutable-global,multi-value,reference-types,saturating-float-to-int,sign-extension

if [ "$(wasm-tools --version 2>/dev/null)" != "wasm-tools $version" ]; then
    cargo install wasm-tools --version "$version" --locked --no-default-features --features validate
fi

if ! wasm-tools validate --features="$features" "$module"; then
    echo "wasm feature floor: $module needs a WebAssembly feature past $features." >&2
    echo "WebKit before the release that added it rejects the whole module." >&2
    echo "Name only these features in the wasm-opt flags of the package's [package.metadata.wasm-pack.profile.release]." >&2
    exit 1
fi
echo "wasm feature floor: $module needs only $features"
