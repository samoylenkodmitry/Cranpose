use cranpose::framebuffer::{Framebuffer, PixelRect, SurfaceError};
use esp_idf_svc::sys::EspError;

use crate::panel::{Panel, PixelFormat, TransferBuffer};

pub(crate) struct Presenter {
    areas: Vec<PixelRect>,
    /// One word a pixel, so the encode loads each pixel's four bytes at once.
    rgba: Vec<u32>,
    /// The buffers the panel reads, in turn: while it reads one, the next
    /// strip is drawn and encoded into the other.
    strips: Vec<TransferBuffer>,
    next_strip: usize,
    strip_pixels: usize,
}

fn rgb565(pixel: u32) -> u16 {
    let [red, green, blue, _] = pixel.to_ne_bytes();
    (u16::from(red & 0xF8) << 8) | (u16::from(green & 0xFC) << 3) | u16::from(blue >> 3)
}

fn encode(rgba: &[u32], strip: &mut [u16], format: PixelFormat) {
    match format {
        PixelFormat::Rgb565 => encode_with(rgba, strip, rgb565),
        PixelFormat::Rgb565BigEndian => {
            encode_with(rgba, strip, |pixel| rgb565(pixel).swap_bytes());
        }
    }
}

/// Writes `encoded` of each pixel of `rgba` into `strip`, four pixels a
/// step: a step of the plain loop spends a third of its instructions on its
/// counter, pointers and branch.
fn encode_with(rgba: &[u32], strip: &mut [u16], encoded: impl Fn(u32) -> u16) {
    let len = rgba.len().min(strip.len());
    let (quads, rest) = rgba[..len].as_chunks::<4>();
    let (out_quads, out_rest) = strip[..len].as_chunks_mut::<4>();
    for (out, quad) in out_quads.iter_mut().zip(quads) {
        *out = quad.map(&encoded);
    }
    for (out, &pixel) in out_rest.iter_mut().zip(rest) {
        *out = encoded(pixel);
    }
}

impl Presenter {
    /// A presenter that draws `strip_rows` rows of `panel` at a time.
    pub(crate) fn new(panel: &Panel<'_>, strip_rows: u32) -> Result<Self, SurfaceError> {
        let strip_pixels = panel.width() as usize * panel.aligned_rows(strip_rows.max(1)) as usize;
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(strip_pixels)
            .map_err(|_| SurfaceError)?;
        rgba.resize(strip_pixels, 0);
        let strips = (0..panel.buffers())
            .map(|_| TransferBuffer::new(strip_pixels))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            areas: Vec::new(),
            rgba,
            strips,
            next_strip: 0,
            strip_pixels,
        })
    }

    pub(crate) fn present(
        &mut self,
        framebuffer: &mut Framebuffer,
        panel: &mut Panel<'_>,
    ) -> Result<(), EspError> {
        framebuffer.changed_areas(&mut self.areas);
        // While the display waits out its start-up, the glyphs of the first
        // picture rasterize into the glyph cache: its strips then take them
        // from there once the display reads pixels.
        if panel.is_setting_up() {
            for &area in &self.areas {
                framebuffer.prepare_area(area);
            }
        }
        for &area in &self.areas {
            let area = panel.align(area);
            let rows_per_strip =
                panel.aligned_rows((self.strip_pixels / area.width.max(1) as usize) as u32);
            let mut top = area.y;
            while top < area.y + area.height {
                let rows = rows_per_strip.min(area.y + area.height - top);
                let strip_area = PixelRect {
                    x: area.x,
                    y: top,
                    width: area.width,
                    height: rows,
                };
                let pixels = strip_area.width as usize * rows as usize;
                framebuffer.draw_area(
                    strip_area,
                    bytemuck::cast_slice_mut(&mut self.rgba[..pixels]),
                );
                panel.wait_for_free_buffer();
                let strip = &mut self.strips[self.next_strip];
                encode(
                    &self.rgba[..pixels],
                    &mut strip.as_mut_slice()[..pixels],
                    panel.format(),
                );
                panel.draw_rect(strip_area, strip)?;
                self.next_strip = (self.next_strip + 1) % self.strips.len();
                top += rows;
            }
        }
        panel.finish_picture()
    }
}
