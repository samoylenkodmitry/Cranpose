// The image a present thread's frame draws into: the surface's next image,
// or one of a test swapchain, which hands its images out in turn and keeps
// what each presented, as a surface that keeps presented images does.

use crate::frame::{ImageContent, PresentedImage};

pub(crate) enum PresentImage {
    Surface(wgpu::SurfaceTexture),
    // The image of the test swapchain at this place.
    Test(usize),
}

pub(crate) struct TestSwapchain {
    images: Vec<wgpu::Texture>,
    // By image, whether it holds what it presented last.
    kept: Vec<bool>,
    next: usize,
}

impl TestSwapchain {
    // `count` images of `format` at `width` by `height` on `device`, none
    // presented yet.
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        (width, height): (u32, u32),
        count: usize,
    ) -> Self {
        let image = |_| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Present Runtime Test Swapchain Image"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: crate::presentable_root_usages(wgpu::TextureUsages::all()),
                view_formats: &[format.remove_srgb_suffix()],
            })
        };
        Self {
            images: (0..count).map(image).collect(),
            kept: vec![false; count],
            next: 0,
        }
    }

    // The next image in turn.
    pub(crate) fn acquire(&mut self) -> PresentImage {
        let index = self.next;
        self.next = (index + 1) % self.images.len();
        PresentImage::Test(index)
    }

    // The texture of `image` and what it holds.
    pub(crate) fn target(&self, index: usize) -> (&wgpu::Texture, ImageContent) {
        let image = PresentedImage {
            index: u32::try_from(index).expect("a test swapchain holds few images"),
            kept: self.kept[index],
        };
        (&self.images[index], ImageContent::Numbered(image))
    }

    // Notes that the image at `index` presented what it holds.
    pub(crate) fn present(&mut self, index: usize) {
        self.kept[index] = true;
    }
}
