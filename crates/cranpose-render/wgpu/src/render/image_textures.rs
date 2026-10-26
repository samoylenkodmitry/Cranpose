use cranpose_core::collections::bounded_lru::BoundedLruCache;
use cranpose_ui_graphics::{ImageBitmap, ImageSampling};

use super::{
    FrameUploads, ImageBinding, MAX_TEXTURE_CACHE_ITEMS,
    image_draws::{CachedImageTexture, ImageBindGroups},
};

const MAX_IMAGE_TEXTURE_CACHE_BYTES: usize = 256 * 1024 * 1024;
const MAX_EXTERNAL_TEXTURE_BINDINGS: usize = 8;

// The renderer's uploaded image textures, each keyed by its image id and
// held to a byte budget, and the bindings of the external textures draws
// sample in place.
pub(crate) struct ImageTextures {
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

    fn get(&mut self, id: u64) -> Option<&CachedImageTexture> {
        self.cache.get(&id)
    }

    // Keeps `texture` for image `id`, then evicts the least recently drawn
    // textures while the cache holds more than its byte budget and more than
    // one texture.
    fn insert(&mut self, id: u64, texture: CachedImageTexture) {
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

    fn external(&mut self, key: u64) -> Option<&ImageBindGroups> {
        self.external.get(&key)
    }

    fn insert_external(&mut self, key: u64, bind_groups: ImageBindGroups) {
        self.external.put(key, bind_groups);
    }

    pub(super) fn len(&self) -> usize {
        self.cache.len()
    }

    pub(super) fn capacity(&self) -> usize {
        self.cache.cap().get()
    }

    /// The binding a draw of `image` sampled by `sampling` takes, uploading
    /// the image the first time a draw needs it.
    pub(crate) fn binding(
        &mut self,
        image: &ImageBitmap,
        sampling: ImageSampling,
        upload: &mut FrameUploads<'_>,
    ) -> Result<ImageBinding, String> {
        if image.format() == cranpose_ui_graphics::ImagePixelFormat::External {
            return self.external_binding(image, sampling, upload);
        }
        if let Some(cached) = self.get(image.id()) {
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

        let texture = upload
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

        let upload_stats = crate::frame_graph::upload_texture(
            upload.queue,
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
        upload.stats.record_command_stats(upload_stats);

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bytes = image.pixels().len();
        let cached = CachedImageTexture {
            bind_groups: upload.pipelines.image_bind_groups(&view, image.format()),
            _texture: texture,
            _view: view,
            bytes,
        };
        let binding = cached.bind_groups.binding(sampling);
        self.insert(image.id(), cached);
        Ok(binding)
    }

    fn external_binding(
        &mut self,
        image: &ImageBitmap,
        sampling: ImageSampling,
        upload: &mut FrameUploads<'_>,
    ) -> Result<ImageBinding, String> {
        let key = image.storage_id();
        if let Some(bind_groups) = self.external(key) {
            return Ok(bind_groups.binding(sampling));
        }
        let view = image
            .external_handle::<wgpu::TextureView>()
            .ok_or("an external texture's handle is not a wgpu::TextureView")?;
        let bind_groups = upload.pipelines.image_bind_groups(view, image.format());
        let binding = bind_groups.binding(sampling);
        self.insert_external(key, bind_groups);
        Ok(binding)
    }
}
