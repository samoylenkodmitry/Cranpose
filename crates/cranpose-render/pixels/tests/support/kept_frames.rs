// A frame a host keeps: it draws only the areas the shell's picture reports
// as changed, and is compared with a full redraw of the scene.

use cranpose_app_shell::AppShell;
use cranpose_render_pixels::{Picture, PixelRect, PixelsRenderer};

pub(crate) struct KeptFrames {
    picture: Picture,
    areas: Vec<PixelRect>,
    kept: Vec<u8>,
    full: Vec<u8>,
    area_pixels: Vec<u8>,
    size: (u32, u32),
    scale: f32,
}

impl KeptFrames {
    pub(crate) fn new(size: (u32, u32), scale: f32) -> Self {
        let bytes = (size.0 * size.1 * 4) as usize;
        Self {
            picture: Picture::default(),
            areas: Vec::new(),
            kept: vec![0; bytes],
            full: vec![0; bytes],
            area_pixels: Vec::new(),
            size,
            scale,
        }
    }

    // Updates `shell` through `steps` steps, `change` making each step after
    // the first, and at every `every`-th update checks the kept frame against
    // a full redraw.
    pub(crate) fn check(
        &mut self,
        shell: &mut AppShell<PixelsRenderer>,
        (steps, every): (u32, u32),
        mut change: impl FnMut(u32),
    ) {
        for step in 0..steps {
            if step > 0 {
                change(step);
            }
            shell.update();
            if step % every != 0 {
                continue;
            }
            let differs = self.take(shell);
            assert_eq!(
                differs, None,
                "step {step} at scale {}: the kept frame differs from a full redraw at (x, y); areas {:?}",
                self.scale, self.areas
            );
        }
    }

    // Brings the kept frame up to the scene of `shell` and returns the first
    // pixel, as (x, y), where it differs from a full redraw of the scene.
    fn take(&mut self, shell: &mut AppShell<PixelsRenderer>) -> Option<(usize, usize)> {
        let (width, height) = self.size;
        shell.primary().with_renderer(|renderer| {
            renderer.update_picture(
                &mut self.picture,
                width,
                height,
                self.scale,
                &mut self.areas,
            );
            for area in &self.areas {
                let row_bytes = area.width as usize * 4;
                self.area_pixels.resize(row_bytes * area.height as usize, 0);
                renderer.draw_picture_area(&self.picture, *area, &mut self.area_pixels);
                for (row, source) in self.area_pixels.chunks_exact(row_bytes).enumerate() {
                    let start = ((area.y as usize + row) * width as usize + area.x as usize) * 4;
                    self.kept[start..start + row_bytes].copy_from_slice(source);
                }
            }
            renderer.draw_scaled(&mut self.full, width, height, self.scale);
        });
        let pixel = self
            .kept
            .as_chunks::<4>()
            .0
            .iter()
            .zip(self.full.as_chunks::<4>().0)
            .position(|(kept, full)| kept != full)?;
        Some((pixel % width as usize, pixel / width as usize))
    }
}
