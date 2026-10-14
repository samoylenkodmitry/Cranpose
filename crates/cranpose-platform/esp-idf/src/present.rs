use cranpose::framebuffer::{Framebuffer, PixelRect, SurfaceError};
use esp_idf_svc::sys::EspError;

use crate::panel::{Panel, PixelFormat, TransferBuffer};

const STRIP_PIXELS: usize = 320 * 24;

pub(crate) struct Presenter {
    areas: Vec<PixelRect>,
    rgba: Vec<u8>,
    strip: TransferBuffer,
    strip_pixels: usize,
}

fn rgb565(pixel: &[u8; 4]) -> u16 {
    (u16::from(pixel[0] & 0xF8) << 8) | (u16::from(pixel[1] & 0xFC) << 3) | u16::from(pixel[2] >> 3)
}

fn encode(rgba: &[u8], strip: &mut [u16], format: PixelFormat) {
    let pixels = strip.iter_mut().zip(rgba.as_chunks::<4>().0);
    match format {
        PixelFormat::Rgb565 => pixels.for_each(|(out, pixel)| *out = rgb565(pixel)),
        PixelFormat::Rgb565BigEndian => {
            pixels.for_each(|(out, pixel)| *out = rgb565(pixel).swap_bytes());
        }
    }
}

impl Presenter {
    pub(crate) fn new(panel: &Panel) -> Result<Self, SurfaceError> {
        let strip_pixels = STRIP_PIXELS.max(panel.width() as usize);
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(strip_pixels * 4)
            .map_err(|_| SurfaceError)?;
        rgba.resize(strip_pixels * 4, 0);
        Ok(Self {
            areas: Vec::new(),
            rgba,
            strip: TransferBuffer::new(strip_pixels)?,
            strip_pixels,
        })
    }

    pub(crate) fn present(
        &mut self,
        framebuffer: &mut Framebuffer,
        panel: &mut Panel,
    ) -> Result<(), EspError> {
        framebuffer.changed_areas(&mut self.areas);
        for area in &self.areas {
            let rows_per_strip = (self.strip_pixels / area.width.max(1) as usize).max(1) as u32;
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
                framebuffer.draw_area(strip_area, &mut self.rgba[..pixels * 4]);
                encode(
                    &self.rgba[..pixels * 4],
                    &mut self.strip.as_mut_slice()[..pixels],
                    panel.format(),
                );
                panel.draw_rect(strip_area, &self.strip)?;
                top += rows;
            }
        }
        Ok(())
    }
}
