use cranpose_core::collections::bounded_lru::BoundedLruCache;
use cranpose_ui_graphics::{ImageBitmap, ImageSampling};

use super::{
    ImageBinding, MAX_TEXTURE_CACHE_ITEMS, RendererPipelines, WgpuFrameGraphExecutor,
    image_draws::{CachedImageTexture, ImageBindGroups},
};
use crate::gpu_stats::FrameStats;

const MAX_IMAGE_TEXTURE_CACHE_BYTES: usize = 256 * 1024 * 1024;
const MAX_EXTERNAL_TEXTURE_BINDINGS: usize = 8;

// The renderer's uploaded image textures, each keyed by its image id and
// held to a byte budget, and the bindings of the external textures draws
// sample in place.
pub(super) struct ImageTextures {
    cache: BoundedLruCache<u64, CachedImageTexture>,
    bytes: usize,
    external: BoundedLruCache<u64, ImageBindGroups>,
}

impl ImageTextures {
    pub(super) fn new() -> Self {
        Self {
            cache: BoundedLruCache::with_capacity_at_least_one(MAX_TEXTURE_CACHE_ITEMS),
            bytes: 0,
            external: BoundedLruCache::with_capacity_at_least_one(MAX_EXTERNAL_TEXTURE_BINDINGS),
        }
    }

    pub(super) fn get(&mut self, id: u64) -> Option<&CachedImageTexture> {
        self.cache.get(&id)
    }

    // Keeps `texture` for image `id`, then evicts the least recently drawn
    // textures while the cache holds more than its byte budget and more than
    // one texture.
    pub(super) fn insert(&mut self, id: u64, texture: CachedImageTexture) {
        let bytes = texture.bytes;
        if let Some(replaced) = self.cache.put(id, texture) {
            self.bytes = self.bytes.saturating_sub(replaced.bytes);
        }
        self.bytes += bytes;
        while self.bytes > MAX_IMAGE_TEXTURE_CACHE_BYTES && self.cache.len() > 1 {
            let Some((_, evicted)) = self.cache.pop_lru() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(evicted.bytes);
        }
    }

    pub(super) fn external(&mut self, key: u64) -> Option<&ImageBindGroups> {
        self.external.get(&key)
    }

    pub(super) fn insert_external(&mut self, key: u64, bind_groups: ImageBindGroups) {
        self.external.put(key, bind_groups);
    }

    pub(super) fn len(&self) -> usize {
        self.cache.len()
    }

    pub(super) fn capacity(&self) -> usize {
        self.cache.cap().get()
    }
}

// What drawing an image takes: the textures uploaded so far, the pipelines
// whose layout its bindings use, and the queue, executor and stats a new
// texture's upload goes through.
pub(crate) struct ImageUploader<'a> {
    pub(super) textures: &'a mut ImageTextures,
    pub(super) pipelines: &'a RendererPipelines,
    pub(super) queue: &'a wgpu::Queue,
    pub(super) executor: &'a mut WgpuFrameGraphExecutor,
    pub(super) stats: &'a FrameStats,
}

impl ImageUploader<'_> {
    /// The binding a draw of `image` sampled by `sampling` takes, uploading
    /// the image the first time a draw needs it.
    pub(crate) fn binding(
        &mut self,
        image: &ImageBitmap,
        sampling: ImageSampling,
    ) -> Result<ImageBinding, String> {
        if image.format() == cranpose_ui_graphics::ImagePixelFormat::External {
            return self.external_binding(image, sampling);
        }
        if let Some(cached) = self.textures.get(image.id()) {
            return Ok(cached.bind_groups.binding(sampling));
        }

        let size = wgpu::Extent3d {
            width: image.width(),
            height: image.height(),
            depth_or_array_layers: 1,
        };

        let alpha_mask = matches!(
            image.format(),
            cranpose_ui_graphics::ImagePixelFormat::Alpha8 { .. }
        );
        let (format, bytes_per_pixel) = if alpha_mask {
            (wgpu::TextureFormat::R8Unorm, 1)
        } else {
            (wgpu::TextureFormat::Rgba8Unorm, 4)
        };

        let texture = self
            .pipelines
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("Image Texture"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });

        let upload_stats = self.executor.upload_texture(
            self.queue,
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            image.pixels(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_pixel * image.width()),
                rows_per_image: Some(image.height()),
            },
            size,
        );
        self.stats.record_command_stats(upload_stats);

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bytes = image.pixels().len();
        let cached = CachedImageTexture {
            bind_groups: self.pipelines.image_bind_groups(&view, alpha_mask),
            _texture: texture,
            _view: view,
            bytes,
        };
        let binding = cached.bind_groups.binding(sampling);
        self.textures.insert(image.id(), cached);
        Ok(binding)
    }

    fn external_binding(
        &mut self,
        image: &ImageBitmap,
        sampling: ImageSampling,
    ) -> Result<ImageBinding, String> {
        let key = image.storage_id();
        if let Some(bind_groups) = self.textures.external(key) {
            return Ok(bind_groups.binding(sampling));
        }
        let view = image
            .external_handle::<wgpu::TextureView>()
            .ok_or("an external texture's handle is not a wgpu::TextureView")?;
        let bind_groups = self.pipelines.image_bind_groups(view, false);
        let binding = bind_groups.binding(sampling);
        self.textures.insert_external(key, bind_groups);
        Ok(binding)
    }
}
