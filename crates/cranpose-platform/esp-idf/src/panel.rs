#![expect(unsafe_code)]

use std::{ptr, ptr::NonNull, slice};

use cranpose::framebuffer::{PixelRect, SurfaceError};
use esp_idf_svc::sys::{self, EspError, esp, esp_lcd_panel_handle_t};

/// The colour encoding a panel reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    /// RGB565 with the low byte first, as QEMU's virtual panel reads it.
    Rgb565,
    /// RGB565 with the high byte first, as SPI panels read it.
    Rgb565BigEndian,
}

/// A display panel driven through ESP-IDF's `esp_lcd`.
pub struct Panel {
    handle: esp_lcd_panel_handle_t,
    width: u32,
    height: u32,
    format: PixelFormat,
}

impl Panel {
    /// Opens QEMU's virtual RGB panel at `width` by `height` pixels, shown by
    /// `idf.py qemu --graphics` or `qemu-system-xtensa -display`. Real
    /// hardware has no such panel and returns `ESP_ERR_NOT_SUPPORTED`.
    pub fn qemu(width: u32, height: u32) -> Result<Self, EspError> {
        let config = sys::esp_lcd_rgb_qemu_config_t {
            width,
            height,
            bpp: sys::esp_lcd_rgb_qemu_bpp_t_RGB_QEMU_BPP_16,
        };
        let mut handle: esp_lcd_panel_handle_t = ptr::null_mut();
        esp!(unsafe { sys::esp_lcd_new_rgb_qemu(&config, &mut handle) })?;
        let panel = Self {
            handle,
            width,
            height,
            format: PixelFormat::Rgb565,
        };
        esp!(unsafe { sys::esp_lcd_panel_init(panel.handle) })?;
        Ok(panel)
    }

    /// Returns the panel width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the panel height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Returns the colour encoding the panel reads.
    pub fn format(&self) -> PixelFormat {
        self.format
    }

    pub(crate) fn draw_rect(
        &mut self,
        area: PixelRect,
        pixels: &TransferBuffer,
    ) -> Result<(), EspError> {
        if area.width as usize * area.height as usize > pixels.len {
            return Err(EspError::from_infallible::<{ sys::ESP_ERR_INVALID_SIZE }>());
        }
        esp!(unsafe {
            sys::esp_lcd_panel_draw_bitmap(
                self.handle,
                area.x as i32,
                area.y as i32,
                (area.x + area.width) as i32,
                (area.y + area.height) as i32,
                pixels.pixels.as_ptr().cast(),
            )
        })
    }
}

pub(crate) struct TransferBuffer {
    pixels: NonNull<u16>,
    len: usize,
}

impl TransferBuffer {
    pub(crate) fn new(len: usize) -> Result<Self, SurfaceError> {
        let caps = sys::MALLOC_CAP_DMA | sys::MALLOC_CAP_INTERNAL;
        let memory = unsafe { sys::heap_caps_calloc(len, size_of::<u16>(), caps) };
        let pixels = NonNull::new(memory.cast::<u16>()).ok_or(SurfaceError)?;
        Ok(Self { pixels, len })
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [u16] {
        unsafe { slice::from_raw_parts_mut(self.pixels.as_ptr(), self.len) }
    }
}

impl Drop for TransferBuffer {
    fn drop(&mut self) {
        unsafe { sys::heap_caps_free(self.pixels.as_ptr().cast()) };
    }
}

impl Drop for Panel {
    fn drop(&mut self) {
        if let Err(error) = esp!(unsafe { sys::esp_lcd_panel_del(self.handle) }) {
            log::warn!("closing the panel failed: {error}");
        }
    }
}
