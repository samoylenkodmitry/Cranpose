# Cranpose render WGPU

GPU renderer for Cranpose scenes, built on `wgpu`. Apps select this backend
with the `renderer-wgpu` feature on `cranpose` for desktop, Android, iOS and
web GPU presentation. The feature is opt-in; the default feature set contains
`embedded-default-font`.

Native builds can add `renderer-wgpu-gles` to compile the GLES fallback.
Android enables GLES support through its platform feature. Backend selection
and surface setup remain the responsibility of the Cranpose platform layer.
