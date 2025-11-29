# RS-Compose Web Demo

This is a web demo of RS-Compose running in the browser using WebAssembly and WebGPU.

## Prerequisites

1. **Rust toolchain** with wasm32 target:
   ```bash
   rustup target add wasm32-unknown-unknown
   ```

2. **wasm-pack**:
   ```bash
   cargo install wasm-pack
   ```

3. **A modern web browser** with WebGPU support:
   - Chrome 113+ or Edge 113+
   - Safari 18+
   - Firefox Nightly (with WebGPU enabled in about:config)

## Building

Run the build script:

```bash
./build.sh
```

Or manually:

```bash
wasm-pack build --target web --out-dir pkg
```

## Running

After building, start a local web server:

```bash
# Using Python
python3 -m http.server 8080

# Or using Node.js
npx serve .

# Or using Rust
cargo install basic-http-server
basic-http-server .
```

Then open http://localhost:8080 in your browser.

## Features Demonstrated

- Declarative UI using the Compose API
- Text rendering with custom fonts
- Layout with Column, Row, and Spacer
- Color and styling
- WebGPU-based rendering
- Mouse/pointer interaction support

## Architecture

The web platform uses:
- **WASM** for running Rust code in the browser
- **WebGPU** for hardware-accelerated rendering
- **wasm-bindgen** for JavaScript interop
- **web-sys** for Web APIs

The architecture is similar to the desktop and Android platforms, with a platform-specific adapter (`compose-platform-web`) that handles browser events and a runtime module that manages the render loop.

## Troubleshooting

### WebGPU not supported

If you see an error about WebGPU not being supported:
- Make sure you're using a compatible browser (see prerequisites)
- Check if WebGPU is enabled in your browser settings
- Try Chrome Canary or Edge Dev for the latest WebGPU implementation

### WASM module fails to load

- Make sure you're serving the files over HTTP (not file://)
- Check the browser console for detailed error messages
- Ensure the build completed successfully without errors
