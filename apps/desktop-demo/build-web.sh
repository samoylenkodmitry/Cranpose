#!/bin/bash

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "Building Cranpose Demo for Web..."
echo ""

# Check if wasm-pack is installed (check common locations)
WASM_PACK=""
if command -v wasm-pack &> /dev/null; then
    WASM_PACK="wasm-pack"
elif [ -f "$HOME/.cargo/bin/wasm-pack" ]; then
    WASM_PACK="$HOME/.cargo/bin/wasm-pack"
elif [ -f "~/.cargo/bin/wasm-pack" ]; then
    WASM_PACK="~/.cargo/bin/wasm-pack"
else
    echo "Error: wasm-pack is not installed or not in PATH"
    echo "Install it with: cargo install wasm-pack"
    echo "Or add ~/.cargo/bin to your PATH"
    exit 1
fi

echo "Using wasm-pack at: $WASM_PACK"

# Avoid version check noise in CI/profile runs.
export WASM_PACK_SKIP_UPDATE_CHECK=1
export WASM_PACK_DISABLE_UPDATE_CHECK=1
WASM_PACK_LOG_LEVEL="${WASM_PACK_LOG_LEVEL:-error}"

# Check if wasm-opt is available (from binaryen) for size optimization
if command -v wasm-opt &> /dev/null; then
    echo "wasm-opt found - binary size optimization enabled"
else
    echo "Warning: wasm-opt not found. Install binaryen for smaller WASM binaries:"
    echo "  Ubuntu/Debian: sudo apt install binaryen"
    echo "  macOS: brew install binaryen"
    echo "  Arch: pacman -S binaryen"
    echo ""
fi

# Build the WASM module with web feature
# Release profile settings from root Cargo.toml will be used:
# - LTO enabled for cross-crate optimization
# - codegen-units=1 for better optimization
# - wasm-opt runs with -Oz for size optimization
echo "Building WASM module (optimized for size)..."

# Run wasm-pack build, don't exit on error so we can handle it
set +e
"$WASM_PACK" --log-level "$WASM_PACK_LOG_LEVEL" build --target web --out-dir pkg --features web,renderer-wgpu --no-default-features
BUILD_RESULT=$?
set -e

if [ $BUILD_RESULT -ne 0 ]; then
    echo ""
    echo "wasm-pack build failed with exit code $BUILD_RESULT"
    echo "WASM size optimization is required for web builds."
    echo "Install binaryen (wasm-opt) and rerun this script."
    echo ""

    if [ "${ALLOW_UNOPTIMIZED_WASM:-0}" = "1" ]; then
        echo "ALLOW_UNOPTIMIZED_WASM=1 set - retrying with --dev (unoptimized)."
        "$WASM_PACK" --log-level "$WASM_PACK_LOG_LEVEL" build --dev --target web --out-dir pkg --features web,renderer-wgpu --no-default-features
        BUILD_RESULT=$?
        if [ $BUILD_RESULT -ne 0 ]; then
            echo "Build failed even with --dev"
            exit 1
        fi
    else
        exit 1
    fi
fi

# Show resulting binary size
if [ -f "pkg/desktop_app_bg.wasm" ]; then
    SIZE=$(du -h pkg/desktop_app_bg.wasm | cut -f1)
    echo ""
    echo "WASM binary size: $SIZE"
fi

echo ""
echo "Build complete! 🎉"
echo ""
echo "To run the demo:"
echo "1. Start a local web server in this directory:"
echo "   python3 -m http.server 8080"
echo "   or"
echo "   npx serve ."
echo ""
echo "2. Open http://localhost:8080 in your browser"
echo ""
echo "Note: WebGPU support is required. Use Chrome 113+, Edge 113+, or Safari 18+"
