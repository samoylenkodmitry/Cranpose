# Web build and browser backends

The desktop demo's WebAssembly build uses wgpu. WebGPU requires browser and
adapter support. The URL query selects the backend:

- default URL: use WebGPU where the browser offers it, WebGL2 otherwise;
- `?backend=gl`: use WebGL2;
- `?backend=webgpu`: use browser WebGPU.

## Build and run

Install the WebAssembly target and `wasm-pack`, then build from the repository
root:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
just web
```

`just web` builds the optimized desktop demo under `apps/desktop-demo/pkg/`.
Serve the app directory. The app's `index.html` loads the built package:

```sh
cd apps/desktop-demo
python3 -m http.server 8080
```

Open <http://localhost:8080>. For a size-optimized build, install Binaryen so
`wasm-opt` is available. `apps/desktop-demo/build-web.sh --fast` selects the
local development profile; `--release` selects the optimized profile.

`apps/desktop-demo/package-web.sh <output-dir>` packages the built WASM and
`index.html` into a content-addressed static site directory.

## Implementation

The backend preference is read by `crates/cranpose/src/web.rs`. wgpu's web
renderer enables its `webgl` feature in
`crates/cranpose-render/wgpu/Cargo.toml`; the runtime chooses browser WebGPU or
WebGL2 according to the URL preference and available adapters.
