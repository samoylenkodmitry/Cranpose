/// Upstream crates Cranpose ships patched under its own names
/// (forks/README.md). They keep the upstream version and publish ahead of
/// the release crates instead of in lockstep with them.
pub(crate) const FORKED_CRATES: &[&str] = &[
    "cranpose-opus-pure",
    "cranpose-wgpu",
    "cranpose-wgpu-core",
    "cranpose-wgpu-core-deps-apple",
    "cranpose-wgpu-core-deps-emscripten",
    "cranpose-wgpu-core-deps-wasm",
    "cranpose-wgpu-core-deps-windows-linux-android",
    "cranpose-wgpu-hal",
];
