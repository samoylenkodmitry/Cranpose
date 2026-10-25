use cranpose::framebuffer::{Framebuffer, PixelRect, SurfaceError};
use esp_idf_svc::sys::EspError;

use crate::panel::{Panel, PixelBuffer, PixelFormat};

pub(crate) struct Presenter {
    areas: Vec<PixelRect>,
    /// One word a pixel, so the encode loads each pixel's four bytes at once.
    rgba: Vec<u32>,
    /// The buffers the panel reads, in turn: while it reads one, the next
    /// strip is drawn and encoded into the other.
    strips: Vec<PixelBuffer>,
    next_strip: usize,
    strip_pixels: usize,
}

/// `pixel`, straight RGBA8 in a little-endian word, as RGB565: red's top
/// five bits, green's top six and blue's top five. Each field comes from the
/// word with one shift and one mask.
fn rgb565(pixel: u32) -> u16 {
    (((pixel & 0xF8) << 8) | ((pixel >> 5) & 0x07E0) | ((pixel >> 19) & 0x1F)) as u16
}

/// [`rgb565`] with its high byte first, as display controllers read it,
/// without swapping the bytes after: the first byte takes red's top five
/// bits and green's top three, the second green's next three and blue's top
/// five.
fn rgb565_big_endian(pixel: u32) -> u16 {
    ((pixel & 0xF8) | ((pixel >> 13) & 0x07) | ((pixel << 3) & 0xE000) | ((pixel >> 11) & 0x1F00))
        as u16
}

fn encode(rgba: &[u32], strip: &mut [u16], format: PixelFormat) {
    match format {
        PixelFormat::Rgb565 => encode_with(rgba, strip, rgb565),
        PixelFormat::Rgb565BigEndian => encode_with(rgba, strip, rgb565_big_endian),
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
            .map(|_| PixelBuffer::for_transfer(strip_pixels))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            areas: Vec::new(),
            rgba,
            strips,
            next_strip: 0,
            strip_pixels,
        })
    }

    // Draws and sends the areas `framebuffer` changed, and returns the
    // bytes it sent.
    pub(crate) fn present(
        &mut self,
        framebuffer: &mut Framebuffer,
        panel: &mut Panel<'_>,
    ) -> Result<usize, EspError> {
        framebuffer.changed_areas(&mut self.areas);
        let ahead = if panel.is_setting_up() {
            self.draw_ahead(framebuffer, panel)
        } else {
            None
        };
        let mut sent = 0;
        for &area in &self.areas {
            let area = panel.align(area);
            sent += area.width as usize * area.height as usize * 2;
            for strip_area in strips(area, self.rows_per_strip(panel, area)) {
                let pixels = strip_area.width as usize * strip_area.height as usize;
                if ahead.is_none() {
                    framebuffer.draw_area(
                        strip_area,
                        bytemuck::cast_slice_mut(&mut self.rgba[..pixels]),
                    );
                }
                panel.wait_for_free_buffer();
                let strip = &mut self.strips[self.next_strip];
                let encoded = &mut strip.as_mut_slice()[..pixels];
                match &ahead {
                    Some(picture) => {
                        let first = strip_area.y as usize * panel.width() as usize;
                        encoded.copy_from_slice(&picture.as_slice()[first..first + pixels]);
                    }
                    None => encode(&self.rgba[..pixels], encoded, panel.format()),
                }
                panel.draw_rect(strip_area, strip)?;
                self.next_strip = (self.next_strip + 1) % self.strips.len();
            }
        }
        panel.finish_picture()?;
        Ok(sent)
    }

    /// Draws the first picture while the display still waits out its
    /// start-up, so that only its transfer is left once the display reads
    /// pixels: whole into PSRAM, when it covers the panel and the chip has
    /// the room, and returns it; else only the glyphs of its text, into the
    /// glyph cache. Out of line, as only the first pictures draw ahead.
    #[inline(never)]
    fn draw_ahead(
        &mut self,
        framebuffer: &mut Framebuffer,
        panel: &Panel<'_>,
    ) -> Option<PixelBuffer> {
        let whole = PixelRect {
            x: 0,
            y: 0,
            width: panel.width(),
            height: panel.height(),
        };
        let picture = match self.areas.as_slice() {
            [area] if panel.align(*area) == whole => {
                PixelBuffer::in_psram(whole.width as usize * whole.height as usize)
            }
            _ => None,
        };
        let Some(mut picture) = picture else {
            for &area in &self.areas {
                framebuffer.prepare_area(area);
            }
            return None;
        };
        for strip_area in strips(whole, self.rows_per_strip(panel, whole)) {
            let pixels = strip_area.width as usize * strip_area.height as usize;
            framebuffer.draw_area(
                strip_area,
                bytemuck::cast_slice_mut(&mut self.rgba[..pixels]),
            );
            let first = strip_area.y as usize * whole.width as usize;
            encode(
                &self.rgba[..pixels],
                &mut picture.as_mut_slice()[first..first + pixels],
                panel.format(),
            );
        }
        Some(picture)
    }

    /// The rows of a strip of `area`: as many as the strip buffers hold, on
    /// the rows the panel takes.
    fn rows_per_strip(&self, panel: &Panel<'_>, area: PixelRect) -> u32 {
        panel.aligned_rows((self.strip_pixels / area.width.max(1) as usize) as u32)
    }
}

/// The strips of `rows` rows that cover `area`, top to bottom.
fn strips(area: PixelRect, rows: u32) -> impl Iterator<Item = PixelRect> {
    let end = area.y + area.height;
    (area.y..end)
        .step_by(rows.max(1) as usize)
        .map(move |top| PixelRect {
            y: top,
            height: rows.min(end - top),
            ..area
        })
}
