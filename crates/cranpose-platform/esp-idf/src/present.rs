use cranpose::framebuffer::{Framebuffer, PixelRect, SurfaceError};
use esp_idf_svc::sys::EspError;

use crate::panel::{Panel, PixelBuffer};

pub(crate) struct Presenter {
    areas: Vec<PixelRect>,
    /// The buffers the panel reads, in turn: while it reads one, the next
    /// strip is drawn into the other, in the pixels the panel takes.
    strips: Vec<PixelBuffer>,
    next_strip: usize,
    strip_pixels: usize,
}

impl Presenter {
    /// A presenter that draws `strip_rows` rows of `panel` at a time.
    pub(crate) fn new(panel: &Panel<'_>, strip_rows: u32) -> Result<Self, SurfaceError> {
        let strip_pixels = panel.width() as usize * panel.aligned_rows(strip_rows.max(1)) as usize;
        let strips = (0..panel.buffers())
            .map(|_| PixelBuffer::for_transfer(strip_pixels))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            areas: Vec::new(),
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
                panel.wait_for_free_buffer();
                let strip = &mut self.strips[self.next_strip];
                let drawn = &mut strip.as_mut_slice()[..pixels];
                match &ahead {
                    Some(picture) => {
                        let first = strip_area.y as usize * panel.width() as usize;
                        drawn.copy_from_slice(&picture.as_slice()[first..first + pixels]);
                    }
                    None => framebuffer.draw_area_rgb565(strip_area, drawn, panel.format()),
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
            let first = strip_area.y as usize * whole.width as usize;
            framebuffer.draw_area_rgb565(
                strip_area,
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
