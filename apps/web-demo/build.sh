#!/bin/bash
set -e

echo "Building RS-Compose Web Demo..."

# Check if wasm-pack is installed
if ! command -v wasm-pack &> /dev/null; then
    echo "Error: wasm-pack is not installed"
    echo "Install it with: cargo install wasm-pack"
    exit 1
fi

# Build the WASM module
echo "Building WASM module..."
wasm-pack build --target web --out-dir pkg

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
