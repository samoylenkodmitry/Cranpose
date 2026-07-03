# Draft upstream issue: allow building wgpu without naga's WGSL frontend

To file against gfx-rs/wgpu (verify against latest trunk first — the dep
tables may have changed since 29.0.4).

---

**Title:** Binary size: `wgpu-core/wgsl` is hardwired for native targets and
`create_render_pipeline` keeps naga reachable even for passthrough-only users

**Summary.** Applications that create every shader through
`create_shader_module_passthrough` (precompiled SPIR-V, Vulkan) still carry
the full naga stack — WGSL frontend, validator, and SPIR-V backend — in their
binaries. Measured on wgpu 29.0.4, opt-level=z, fat LTO, lld `--icf=all`:
~0.6 MiB of a 3.4 MiB GUI binary is naga that never executes.

**Cause 1 — manifest:** `wgpu`'s native dependency table enables
`wgpu-core/wgsl` (and `renderdoc`) unconditionally:

```toml
[target.'cfg(not(target_arch = "wasm32"))'.dependencies.wgpu-core]
features = ["renderdoc", "wgsl", "portable-atomic"]
```

so disabling the `wgsl` feature on `wgpu` itself does not remove
`naga/wgsl-in` from the build graph.

**Cause 2 — reachability:** even with all modules created via passthrough,
`wgpu_core::device::Device::create_render_pipeline` dispatches on the shader
module's kind at runtime, so the naga-consuming pipeline path (validator +
`back::spv`) remains reachable code and no linker can strip it. Verified by
building a real renderer entirely on `PASSTHROUGH_SHADERS` (all pixel tests
passing) and observing a net binary-size *increase* (+0.07 MB — blobs added,
nothing removed).

**Ask.**
1. Stop force-enabling `wgpu-core/wgsl` from `wgpu`'s dep tables; let the
   existing `wgpu/wgsl` feature control it end to end.
2. Gate the naga-consuming arm of pipeline creation (or the whole
   naga-module representation) behind the shader-frontend features, so a
   passthrough-only + explicit-layout application can compile `wgpu-core`
   without naga at all.

**Impact.** For GUI-framework hello-world binaries in the 3–4 MB range, naga
is the single largest removable dependency (~15–20%). Passthrough users
already accept validation at build time; they get no benefit from the
runtime copy.
