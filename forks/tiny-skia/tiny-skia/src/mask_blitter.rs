// Copyright 2026 Cranpose contributors
//
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! A blitter that draws white ink straight into a mask's bytes, with the
//! integer arithmetic of the low-precision raster pipeline: a mask drawn here
//! holds the bytes the alpha channel of a pixmap gets from the same white
//! paint, and no raster pipeline is built.

use crate::alpha_runs::AlphaRun;
use crate::blitter::Blitter;
use crate::color::AlphaU8;
use crate::geom::ScreenIntRect;
use crate::pixmap::SubPixmapMut;
use crate::{LengthU32, ALPHA_U8_OPAQUE, ALPHA_U8_TRANSPARENT};

/// The white ink a [`MaskBlitter`] draws with.
#[derive(Clone, Copy)]
pub(crate) enum Ink {
    /// An opaque paint: the pipeline lerps toward it by the coverage, and
    /// fills whole spans with 255.
    Opaque,
    /// A paint of this alpha (0 to 255): the pipeline scales it by the
    /// coverage, then draws it source-over.
    Translucent(u16),
}

impl Ink {
    /// The ink of a paint of `opacity`, as the pipeline loads its uniform
    /// color.
    pub(crate) fn of_opacity(opacity: f32) -> Self {
        if opacity >= 1.0 {
            Ink::Opaque
        } else {
            Ink::Translucent((opacity * 255.0 + 0.5) as u16)
        }
    }

    // The byte over `d` where the ink covers `c` of 255.
    #[inline]
    fn over(self, d: u8, c: u16) -> u8 {
        let d = u32::from(d);
        let c = u32::from(c);
        let value = match self {
            Ink::Opaque => div255(d * (255 - c) + 255 * c),
            Ink::Translucent(alpha) => {
                let s = div255(u32::from(alpha) * c);
                s + div255(d * (255 - s))
            }
        };
        value as u8
    }

    // The byte over `d` where the ink covers the pixel whole.
    #[inline]
    fn over_whole(self, d: u8) -> u8 {
        match self {
            Ink::Opaque => 255,
            Ink::Translucent(alpha) => {
                let s = u32::from(alpha);
                (s + div255(u32::from(d) * (255 - s))) as u8
            }
        }
    }
}

// The pipeline's rounding of a product of two bytes back to a byte.
#[inline]
fn div255(v: u32) -> u32 {
    (v + 255) >> 8
}

// The coverage the pipeline takes from a run's alpha: as a float, then back.
#[inline]
fn run_coverage(alpha: AlphaU8) -> u16 {
    let coverage = alpha as f32 * (1.0 / 255.0);
    (coverage * 255.0 + 0.5) as u16
}

pub(crate) struct MaskBlitter<'a, 'b> {
    pixmap: &'a mut SubPixmapMut<'b>,
    ink: Ink,
}

impl<'a, 'b> MaskBlitter<'a, 'b> {
    pub(crate) fn new(pixmap: &'a mut SubPixmapMut<'b>, ink: Ink) -> Self {
        MaskBlitter { pixmap, ink }
    }

    #[inline]
    fn pixel(&mut self, x: u32, y: u32) -> &mut u8 {
        let offset = self.pixmap.real_width * y as usize + x as usize;
        &mut self.pixmap.data[offset]
    }

    #[inline]
    fn cover(&mut self, x: u32, y: u32, coverage: u16) {
        let ink = self.ink;
        let pixel = self.pixel(x, y);
        *pixel = ink.over(*pixel, coverage);
    }
}

impl Blitter for MaskBlitter<'_, '_> {
    fn blit_h(&mut self, x: u32, y: u32, width: LengthU32) {
        let start = self.pixmap.real_width * y as usize + x as usize;
        let ink = self.ink;
        for pixel in &mut self.pixmap.data[start..start + width.get() as usize] {
            *pixel = ink.over_whole(*pixel);
        }
    }

    fn blit_anti_h(&mut self, mut x: u32, y: u32, aa: &mut [AlphaU8], runs: &mut [AlphaRun]) {
        let mut offset = 0;
        let mut run_opt = runs[0];
        while let Some(run) = run_opt {
            let width = LengthU32::from(run);
            match aa[offset] {
                ALPHA_U8_TRANSPARENT => {}
                ALPHA_U8_OPAQUE => self.blit_h(x, y, width),
                alpha => {
                    let coverage = run_coverage(alpha);
                    for dx in 0..width.get() {
                        self.cover(x + dx, y, coverage);
                    }
                }
            }
            x += width.get();
            offset += usize::from(run.get());
            run_opt = runs[offset];
        }
    }

    fn blit_v(&mut self, x: u32, y: u32, height: LengthU32, alpha: AlphaU8) {
        for dy in 0..height.get() {
            self.cover(x, y + dy, u16::from(alpha));
        }
    }

    fn blit_anti_h2(&mut self, x: u32, y: u32, alpha0: AlphaU8, alpha1: AlphaU8) {
        self.cover(x, y, u16::from(alpha0));
        self.cover(x + 1, y, u16::from(alpha1));
    }

    fn blit_anti_v2(&mut self, x: u32, y: u32, alpha0: AlphaU8, alpha1: AlphaU8) {
        self.cover(x, y, u16::from(alpha0));
        self.cover(x, y + 1, u16::from(alpha1));
    }

    fn blit_rect(&mut self, rect: &ScreenIntRect) {
        for dy in 0..rect.height() {
            self.blit_h(rect.x(), rect.y() + dy, rect.width_safe());
        }
    }
}
