# Cranpose render pixels

Software renderer for Cranpose scenes. The backend rasterizes scene geometry
and text into an RGBA byte buffer on the CPU. Cranpose uses this renderer for
the experimental watchOS host; applications can also select the
`renderer-pixels` feature on `cranpose`.

The renderer exposes `PixelsRenderer`, its `Scene`, and `draw`/`draw_scaled`
methods. Host code owns the output buffer and presents its pixels.
