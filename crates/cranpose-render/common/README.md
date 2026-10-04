# Cranpose render common

Shared render contracts serve Cranpose backends. `RenderScene` exposes
the collected scene and hit-test data, while `Renderer` rebuilds a scene from
the UI layout or composition applier and can install renderer services into
an app context. The concrete backends are `cranpose-render-wgpu` and
`cranpose-render-pixels`.

Applications normally select a renderer through features on `cranpose`;
implement this crate's traits for a custom renderer.
