use std::mem;

use cranpose::framebuffer::SurfaceError;
use esp_idf_svc::sys::EspError;

use crate::panel::{Panel, PixelFormat, TransferBuffer};

const BAND_BYTES: usize = 32 * 1024;

pub(crate) struct Presenter {
    current: Vec<u16>,
    previous: Vec<u16>,
    band: TransferBuffer,
    band_rows: usize,
    presented: bool,
}

fn frame(pixels: usize) -> Result<Vec<u16>, SurfaceError> {
    let mut frame = Vec::new();
    frame.try_reserve_exact(pixels).map_err(|_| SurfaceError)?;
    frame.resize(pixels, 0);
    Ok(frame)
}

fn rgb565(pixel: &[u8; 4]) -> u16 {
    (u16::from(pixel[0] & 0xF8) << 8) | (u16::from(pixel[1] & 0xFC) << 3) | u16::from(pixel[2] >> 3)
}

impl Presenter {
    pub(crate) fn new(panel: &Panel) -> Result<Self, SurfaceError> {
        let width = panel.width() as usize;
        let height = panel.height() as usize;
        let pixels = width.checked_mul(height).ok_or(SurfaceError)?;
        let band_rows = (BAND_BYTES / (width * size_of::<u16>())).clamp(1, height);
        Ok(Self {
            current: frame(pixels)?,
            previous: frame(pixels)?,
            band: TransferBuffer::new(band_rows * width)?,
            band_rows,
            presented: false,
        })
    }

    pub(crate) fn present(&mut self, panel: &mut Panel, rgba: &[u8]) -> Result<(), EspError> {
        let width = panel.width() as usize;
        let changed = match panel.format() {
            PixelFormat::Rgb565 => self.convert(rgba, width, rgb565),
            PixelFormat::Rgb565BigEndian => {
                self.convert(rgba, width, |pixel| rgb565(pixel).swap_bytes())
            }
        };
        let Some((top, bottom)) = changed else {
            return Ok(());
        };
        for band_top in (top..bottom).step_by(self.band_rows) {
            let rows = self.band_rows.min(bottom - band_top);
            let pixels = band_top * width..(band_top + rows) * width;
            self.band.as_mut_slice()[..pixels.len()].copy_from_slice(&self.current[pixels]);
            panel.draw_rows(band_top, rows, &self.band)?;
        }
        mem::swap(&mut self.current, &mut self.previous);
        self.presented = true;
        Ok(())
    }

    fn convert(
        &mut self,
        rgba: &[u8],
        width: usize,
        encode: impl Fn(&[u8; 4]) -> u16,
    ) -> Option<(usize, usize)> {
        let mut changed: Option<(usize, usize)> = None;
        let rows = self
            .current
            .chunks_exact_mut(width)
            .zip(self.previous.chunks_exact(width))
            .zip(rgba.as_chunks::<4>().0.chunks_exact(width));
        for (row, ((current, previous), source)) in rows.enumerate() {
            for (pixel, rgba) in current.iter_mut().zip(source) {
                *pixel = encode(rgba);
            }
            if !self.presented || current != previous {
                changed = Some((changed.map_or(row, |(top, _)| top), row + 1));
            }
        }
        changed
    }
}
