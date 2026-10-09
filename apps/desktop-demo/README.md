# Cranpose Demo

This demo application showcases Cranpose on **Desktop**, **Android**, **iOS**, and **Web**.

## Features Demonstrated

- Interactive counter app
- Composition locals
- Async runtime and effects
- Web data fetching
- Recursive layouts
- Modifier showcase
- Mineswapper 2 game
- Animations and state management

## Building & Running

### Desktop

Run the desktop demo:

```bash
cargo run -p desktop-app --bin desktop-app
```

#### macOS packaging

Distribute the macOS demo as a `.app` bundle, never as a bare executable: macOS
Finder runs bare Unix executables inside a Terminal window, so double-clicking a
loose binary spawns a terminal alongside (or instead of) the GUI.

Build a signed bundle with xtask:

```bash
cargo xtask bundle-macos --target aarch64-apple-darwin
```

The bundle shows `assets/icon/CranposeDemo.icns` as its icon; `--icon` names
another `.icns` file.

The bundle is sealed with an ad-hoc signature by default (`_CodeSignature/CodeResources`
binding `Info.plist`). This is required: an unsealed bundle is reported as
"is damaged and should be moved to the Trash" by Gatekeeper once it has been
downloaded (quarantined). Without a paid Apple Developer ID the app is still
unidentified, so the first launch needs right-click → **Open** (or pass
`--sign-identity "Developer ID Application: …"` to sign for distribution).

### Android

The Android host is under [`apps/android-demo/android`](../android-demo/android/).
From the repository root, `just android` builds the demo APK; see the Android
Gradle files and `just android` recipe for the current build configuration.

### Web

The demo uses WebGPU where the browser offers it and WebGL2 otherwise. Add
`?backend=gl` or `?backend=webgpu` to the URL to force one.

1. **Prerequisites:**
   ```bash
   rustup target add wasm32-unknown-unknown
   cargo install wasm-pack
   ```

2. **Build:**
   ```bash
   just web
   # Or build this demo directly:
   ./apps/desktop-demo/build-web.sh --release
   ```

3. **Run:**
   ```bash
   # Using Python
   cd apps/desktop-demo && python3 -m http.server 8080

   # Or using Node.js
   npx serve apps/desktop-demo

   # Or using Rust
   cargo install basic-http-server
   basic-http-server apps/desktop-demo
   ```

4. **Open** http://localhost:8080 in a browser with WebGL2 support

### App icon

[`assets/icon/icon.svg`](assets/icon/icon.svg) is the demo's icon on every
platform, and the web page serves it as its favicon. The other platforms package
pictures of it, which `node scripts/dev/render_app_icons.mjs` draws through
headless Chrome: the Windows and Linux window icon, the macOS `.icns`, the iOS
icon files and the Android launcher icons. Run it after a change to the SVG and
commit the files it writes.

## Architecture

This application demonstrates the cross-platform nature of Cranpose:

- **Single codebase** for all platforms
- **Platform-specific entry points** (`main.rs` for desktop, `desktop-demo-platform` for Android/Web, `ios_main.rs` for iOS)
- **Shared UI code** in `app.rs` using composable functions
- **Platform detection** using conditional compilation

### Code Structure

```
desktop-demo/
├── src/
│   ├── main.rs          # Desktop entry point
│   ├── lib.rs           # Shared demo UI library
│   ├── ios_main.rs      # iOS entry point
│   ├── app.rs           # Main UI composables
│   ├── fonts.rs         # Embedded fonts
│   └── tests/           # Tests
├── index.html           # Web HTML template
├── build-web.sh         # Web build script
└── Cargo.toml           # Multi-platform dependencies
```

The Android and Web platform wrappers live in the sibling
[`desktop-demo-platform`](../desktop-demo-platform/) package.

## Troubleshooting

### Desktop

If you encounter rendering issues:
- Update your graphics drivers
- Try the pixels renderer: `cargo run -p desktop-app --bin desktop-app --no-default-features --features desktop,renderer-pixels`

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
