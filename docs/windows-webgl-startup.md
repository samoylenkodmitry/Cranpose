# Windows WebGL startup

Firefox on Windows stalled while linking Cranpose's first shape program. The
captured vertex shader indexed a uniform array of 64 `Placement` structs, each
containing a color matrix. Shader compilation reported success, but linking
did not complete within 30 seconds. The full app could remain blank for minutes.

On Windows 11 (build 26100), Firefox 143, and an NVIDIA RTX 2070 (driver
30.0.15.1228), changing only the captured placement array from 64 entries to four
made the same program link and service animation frames in 930 ms. Reducing the
brush and gradient-stop tables was unnecessary.

The uniform-backed arena now closes a chunk at four placements. Its buffer
binding size and WGSL declaration use the same capacity. Storage-backed arenas
remain unbounded and retain the existing batching. General shape pipelines are
prepared eagerly only when the asynchronous specialization path needs their
fallbacks; synchronous backends compile the pipelines they actually draw.

Validation:

- Cranamp on Windows Firefox rendered its player and serviced animation frames
  in 1.2 seconds with the fixed framework, using the default WebGL backend.
- Two placement-boundary/binding tests failed with the old capacity and passed
  with the new capacity.
- The renderer suite on macOS Metal passed 499 tests, including all six shape
  specialization parity tests; nine existing native-parity tests were ignored.
- Renderer clippy and workspace formatting passed.

The Windows native release had a separate consumer-build defect:
`-Zfmt-debug=none` erased floating-point literals written by Naga's HLSL backend.
Applications must preserve Debug formatting in GPU builds. Cranamp's release
workflow now does so; its original Windows artifact failed a window-pixel smoke
test, while the corrected size-optimized release passed on the same machine.
