# Web Build Requirements

## Browser Compatibility

The RS-Compose web demo currently requires a browser with **updated WebGPU support** due to a field name incompatibility between wgpu 0.19 and Chrome stable.

### Issue

wgpu 0.19 uses the newer WebGPU specification field name `maxInterStageShaderComponents`, while Chrome stable (as of version 113-131) still uses the older field name `maxInterStageShaderVariables`. This causes device creation to fail with:

```
OperationError: Failed to execute 'requestDevice' on 'GPUAdapter':
The limit "maxInterStageShaderComponents" with a non-undefined value is not recognized.
```

### Solution

Use one of the following browsers with updated WebGPU support:

1. **Chrome Canary** (recommended)
   - Download from: https://www.google.com/chrome/canary/
   - Enable WebGPU in `chrome://flags/#enable-unsafe-webgpu`
   - Restart browser

2. **Chrome Dev**
   - Download from: https://www.google.com/chrome/dev/
   - Enable WebGPU in `chrome://flags/#enable-unsafe-webgpu`
   - Restart browser

3. **Microsoft Edge Canary/Dev**
   - Similar process to Chrome

4. **Safari Technology Preview** (macOS only)
   - Download from: https://developer.apple.com/safari/technology-preview/
   - WebGPU enabled by default

### Why Can't We Downgrade wgpu?

The project uses `glyphon 0.5` for text rendering, which requires `wgpu 0.19`. Downgrading to wgpu 0.17 or earlier would break text rendering and require finding compatible versions of all graphics dependencies.

### Future

This issue will be resolved when:
- Chrome stable updates to support the newer WebGPU specification field names, OR
- wgpu adds compatibility shims for older browsers

## Building for Web

```bash
cd apps/desktop-demo
./build-web.sh

# Start a local server
python3 -m http.server 8080

# Open in Chrome Canary/Dev
# http://localhost:8080
```

## Verification

To verify your browser has proper WebGPU support:
1. Visit https://webgpureport.org/
2. Check that "maxInterStageShaderComponents" is listed in the limits
3. If you only see "maxInterStageShaderVariables", your browser is too old

## Technical Details

The incompatibility occurs during `adapter.requestDevice()` when wgpu serializes the `Limits` struct to JavaScript. Regardless of which limits we specify in Rust (default, downlevel_webgl2_defaults, adapter.limits(), etc.), wgpu 0.19 always serializes using its struct field names, which include `max_inter_stage_shader_components` → JavaScript `maxInterStageShaderComponents`.

Chrome's WebGPU implementation rejects any limit name it doesn't recognize, even if the value would be acceptable.
