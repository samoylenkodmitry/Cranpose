# RS-Compose Demo

This is a comprehensive demo application showcasing RS-Compose running on multiple platforms: **Desktop**, **Android**, and **Web**.

## Features Demonstrated

- Interactive counter app
- Composition locals
- Async runtime and effects
- Web data fetching
- Recursive layouts
- Modifier showcase
- Mineswapper game
- Animations and state management

## Building & Running

### Desktop

Run the desktop demo:

```bash
cargo run --bin desktop-app
```

Or from the repository root:

```bash
cargo run --bin desktop-app
```

### Android

This app is used by the Android demo. See [`apps/android-demo/README.md`](../android-demo/README.md) for build instructions.

### Web

The demo runs in any modern browser using WebGL2 (Chrome, Firefox, Edge, Safari).

1. **Prerequisites:**
   ```bash
   rustup target add wasm32-unknown-unknown
   cargo install wasm-pack
   ```

2. **Build:**
   ```bash
   ./build-web.sh
   ```

3. **Run:**
   ```bash
   # Using Python
   python3 -m http.server 8080

   # Or using Node.js
   npx serve .

   # Or using Rust
   cargo install basic-http-server
   basic-http-server .
   ```

4. **Open** http://localhost:8080 in any modern browser

## Architecture

This application demonstrates the cross-platform nature of RS-Compose:

- **Single codebase** for all platforms
- **Platform-specific entry points** (main.rs for desktop, lib.rs exports for Android/Web)
- **Shared UI code** in `app.rs` using composable functions
- **Platform detection** using conditional compilation

### Code Structure

```
desktop-demo/
├── src/
│   ├── main.rs          # Desktop entry point
│   ├── lib.rs           # Shared library with Android & Web entry points
│   ├── app.rs           # Main UI composables
│   ├── fonts.rs         # Embedded fonts
│   └── tests/           # Tests
├── index.html           # Web HTML template
├── build-web.sh         # Web build script
└── Cargo.toml           # Multi-platform dependencies
```

## Troubleshooting

### Desktop

If you encounter rendering issues:
- Update your graphics drivers
- Try the pixels renderer: `cargo run --bin desktop-app --features renderer-pixels --no-default-features`

### Web

**WebGL2 not supported:**
- Update your browser to the latest version
- Enable WebGL in browser settings
- Update your graphics drivers

**WASM module fails to load:**
- Serve files over HTTP (not file://)
- Check browser console for detailed errors
- Ensure build completed without errors

**Blank canvas or no rendering:**
- Check browser console for errors
- Verify WebGL2 is available: visit https://get.webgl.org/webgl2/
- Try a different browser

**Performance issues:**
- WebGL is hardware-accelerated, but may be slower than native
- Check browser DevTools for performance profiling
