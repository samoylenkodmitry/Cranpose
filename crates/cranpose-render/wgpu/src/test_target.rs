// The target a present thread draws into in place of a surface in a test:
// a new texture for each frame, which holds nothing known, or a test
// swapchain, which hands its images out in turn and keeps what each
// presented, as a surface that keeps presented images does.

use crate::frame::{ImageContent, PresentedImage};

/// How a test target hands out the images its frames draw into.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestImages {
    /// A new texture for each frame, which holds nothing known.
    Fresh,
    /// So many images in turn, each holding what it presented, as a
    /// swapchain that keeps presented images does.
    Kept(usize),
}

pub(crate) struct TestTarget {
    pub(crate) size: (u32, u32),
    // `None` draws each frame into a new texture.
    swapchain: Option<TestSwapchain>,
}

impl TestTarget {
    // A target of `size` with `images` of `format` on `device`.
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        size: (u32, u32),
        images: TestImages,
    ) -> Self {
        let swapchain = match images {
            TestImages::Fresh => None,
            TestImages::Kept(count) => Some(TestSwapchain::new(device, format, size, count.max(1))),
        };
        Self { size, swapchain }
    }

    pub(crate) fn swapchain(&mut self) -> Option<&mut TestSwapchain> {
        self.swapchain.as_mut()
    }

    // The texture of the swapchain image at `index` and what it holds.
    pub(crate) fn image(&self, index: usize) -> (&wgpu::Texture, ImageContent) {
        self.swapchain
            .as_ref()
            .expect("a test image needs its swapchain")
            .target(index)
    }

    // A new texture of the target's size that holds nothing known.
    pub(crate) fn fresh_texture(
        &self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Present Runtime Offscreen Test Target"),
            size: wgpu::Extent3d {
                width: self.size.0,
                height: self.size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
    }
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
    fn new(
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

    // The index of the next image in turn.
    pub(crate) fn acquire(&mut self) -> usize {
        let index = self.next;
        self.next = (index + 1) % self.images.len();
        index
    }

    // The texture of the image at `index` and what it holds.
    fn target(&self, index: usize) -> (&wgpu::Texture, ImageContent) {
        let few = |count: usize| u32::try_from(count).expect("a test swapchain holds few images");
        let image = PresentedImage {
            index: few(index),
            kept: self.kept[index],
            images: few(self.images.len()),
        };
        (&self.images[index], ImageContent::Numbered(image))
    }

    // Notes that the image at `index` presented what it holds.
    pub(crate) fn present(&mut self, index: usize) {
        self.kept[index] = true;
    }
}
