use std::{hash::Hash, rc::Rc};

use cranpose_core::collections::bounded_lru::BoundedLruCache;
use cranpose_render_common::{
    glyph_mask::{GlyphMask, GlyphMaskKey},
    text_atlas::AtlasGlyph,
    text_mask_gamma::TextLuminance,
};
use cranpose_ui_graphics::{BlendMode, Color};

use super::{
    FrameUploads, GlyphInstance, GlyphQuads, ImageUvRect, PassDepth, TextResources, TurnedGlyph,
    blend_state_for_mode, create_render_pipeline_logged,
};
use crate::{
    geometry::SegmentTransform,
    gpu_stats::{self},
    shared_shader::SharedShader,
};

const MAX_TEXT_GLYPH_ATLAS_ITEMS: usize = 8192;

pub(super) const TEXT_GLYPH_ATLAS_MIN_SIZE: u32 = 512;

const TEXT_GLYPH_ATLAS_MAX_SIZE: u32 = 4096;

const _: () = assert!(TEXT_GLYPH_ATLAS_MAX_SIZE <= u16::MAX as u32);

const TEXT_GLYPH_ATLAS_PADDING: u32 = 1;

pub(super) fn create_glyph_atlas_pipeline(
    device: &wgpu::Device,
    cache: Option<&wgpu::PipelineCache>,
    surface_format: wgpu::TextureFormat,
    shader: &SharedShader,
    (depth, quads): (PassDepth, GlyphQuads),
) -> wgpu::RenderPipeline {
    let module = shader.module();
    create_render_pipeline_logged(
        device,
        cache,
        match (depth, quads) {
            (PassDepth::Off, GlyphQuads::Plain) => "glyph-atlas",
            (PassDepth::On, GlyphQuads::Plain) => "glyph-atlas depth",
            (PassDepth::Off, GlyphQuads::Aligned) => "glyph-atlas aligned",
            (PassDepth::On, GlyphQuads::Aligned) => "glyph-atlas aligned depth",
            (PassDepth::Off, GlyphQuads::Turned) => "glyph-atlas turned",
            (PassDepth::On, GlyphQuads::Turned) => "glyph-atlas turned depth",
        },
        wgpu::RenderPipelineDescriptor {
            label: Some("Glyph Atlas Pipeline"),
            layout: Some(shader.layout()),
            vertex: wgpu::VertexState {
                module,
                entry_point: Some(match quads {
                    GlyphQuads::Plain => "glyph_atlas_vs_main",
                    GlyphQuads::Aligned => "glyph_atlas_aligned_vs_main",
                    GlyphQuads::Turned => "glyph_atlas_turned_vs_main",
                }),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(match quads {
                    GlyphQuads::Turned => TurnedGlyph::desc(),
                    GlyphQuads::Plain | GlyphQuads::Aligned => GlyphInstance::desc(),
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module,
                entry_point: Some(match quads {
                    GlyphQuads::Aligned => "glyph_atlas_aligned_fs_main",
                    GlyphQuads::Plain | GlyphQuads::Turned => "glyph_atlas_fs_main",
                }),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(blend_state_for_mode(BlendMode::SrcOver)),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: depth.overlay_state(),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        },
    )
}

#[derive(Clone, Copy)]
pub(super) struct GlyphAtlasEntry {
    pub(super) x: u16,
    pub(super) y: u16,
    pub(super) width: u16,
    pub(super) height: u16,
}

fn next_glyph_atlas_size(current: u32, max: u32) -> u32 {
    current.saturating_mul(2).clamp(1, max.max(1))
}

/// The samplers the glyph atlas binds: nearest for glyphs on the pixel grid,
/// linear for glyphs a segment transform turns.
#[derive(Clone, Copy)]
pub(super) struct GlyphSamplers<'a> {
    pub(super) nearest: &'a wgpu::Sampler,
    pub(super) linear: &'a wgpu::Sampler,
}

/// A glyph's atlas slot: its mask corrected for the luminance class of the
/// text it is drawn in, so one glyph in light and dark text takes two slots.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct GlyphAtlasSlotKey {
    glyph: GlyphMaskKey,
    luminance: TextLuminance,
}

impl GlyphAtlasSlotKey {
    pub(super) fn new(glyph: GlyphMaskKey, color: Color) -> Self {
        Self {
            glyph,
            luminance: TextLuminance::of_color(color),
        }
    }
}

/// Slots of [`RunAtlasEntries`], by glyph id.
const RUN_ATLAS_ENTRY_SLOTS: usize = 64;

/// The atlas entries a run's preparation has looked up, by glyph id: a run
/// repeats its letters, and each atlas lookup hashes the slot key and moves
/// the entry up the atlas's recency order, where the first lookup already
/// put it for this frame.
pub(super) struct RunAtlasEntries {
    slots: [Option<(GlyphAtlasSlotKey, GlyphAtlasEntry)>; RUN_ATLAS_ENTRY_SLOTS],
}

impl RunAtlasEntries {
    pub(super) fn new() -> Self {
        Self {
            slots: [None; RUN_ATLAS_ENTRY_SLOTS],
        }
    }

    /// `glyph`'s entry from an earlier glyph of the run, else from
    /// `look_up`, remembered for the rest of it.
    pub(super) fn entry<'u>(
        &mut self,
        (text, upload): (&mut TextResources, &mut FrameUploads<'u>),
        glyph: &AtlasGlyph,
        look_up: impl FnOnce(
            &mut TextResources,
            &mut FrameUploads<'u>,
        ) -> Result<GlyphAtlasEntry, String>,
    ) -> Result<GlyphAtlasEntry, String> {
        let key = GlyphAtlasSlotKey::new(glyph.key, glyph.color);
        let slot = &mut self.slots[glyph.key.glyph_id as usize % RUN_ATLAS_ENTRY_SLOTS];
        if let Some((seen, entry)) = *slot
            && seen == key
        {
            upload.stats.record_text_glyph_atlas_hits(1);
            return Ok(entry);
        }
        let entry = look_up(text, upload)?;
        *slot = Some((key, entry));
        Ok(entry)
    }
}

pub(super) struct TextGlyphAtlas {
    pub(super) texture: wgpu::Texture,
    _view: wgpu::TextureView,
    pub(super) texel_bind_group: Rc<wgpu::BindGroup>,
    pub(super) filtered_bind_group: Rc<wgpu::BindGroup>,
    entries: BoundedLruCache<GlyphAtlasSlotKey, GlyphAtlasEntry>,
    generation: u64,
    size: u32,
    max_size: u32,
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
    /// The glyphs placed since [`Self::flush_uploads`], in the order they
    /// were placed, and their coverage one after another.
    pending: Vec<GlyphAtlasEntry>,
    pending_coverage: Vec<u8>,
    upload_scratch: Vec<u8>,
}

impl TextGlyphAtlas {
    pub(super) fn new(
        device: &wgpu::Device,
        image_layout: &wgpu::BindGroupLayout,
        samplers: GlyphSamplers<'_>,
        size: u32,
    ) -> Self {
        // The side stays a power of two, whose texel is exact: see
        // [`TextGlyphAtlas::texel`].
        let device_max = device.limits().max_texture_dimension_2d.max(1);
        let max_size = TEXT_GLYPH_ATLAS_MAX_SIZE.min(1 << device_max.ilog2());
        let size = size
            .next_power_of_two()
            .clamp(TEXT_GLYPH_ATLAS_MIN_SIZE.min(max_size), max_size);
        let texture = Self::create_texture(device, size);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = |label: &'static str, sampler: &wgpu::Sampler| {
            Rc::new(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: image_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            }))
        };
        let texel_bind_group = bind("Text Glyph Atlas Bind Group", samplers.nearest);
        let filtered_bind_group = bind("Filtered Text Glyph Atlas Bind Group", samplers.linear);
        Self {
            texture,
            _view: view,
            texel_bind_group,
            filtered_bind_group,
            entries: BoundedLruCache::with_capacity_at_least_one(MAX_TEXT_GLYPH_ATLAS_ITEMS),
            generation: 0,
            size,
            max_size,
            cursor_x: TEXT_GLYPH_ATLAS_PADDING,
            cursor_y: TEXT_GLYPH_ATLAS_PADDING,
            row_height: 0,
            pending: Vec::new(),
            pending_coverage: Vec::new(),
            upload_scratch: Vec::new(),
        }
    }

    fn create_texture(device: &wgpu::Device, size: u32) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Text Glyph Atlas Texture"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    pub(crate) fn reset(
        &mut self,
        device: &wgpu::Device,
        image_layout: &wgpu::BindGroupLayout,
        samplers: GlyphSamplers<'_>,
    ) {
        let generation = self.generation.wrapping_add(1);
        let grown = next_glyph_atlas_size(self.size, self.max_size);
        let mut next = Self::new(device, image_layout, samplers, grown);
        next.generation = generation;
        *self = next;
    }

    /// The atlas bound for glyphs drawn under `transform`: texel for texel
    /// on the pixel grid, filtered once a transform turns the glyph quads
    /// off it.
    pub(crate) fn bind_group(&self, transform: SegmentTransform) -> Rc<wgpu::BindGroup> {
        Rc::clone(if transform.is_identity() {
            &self.texel_bind_group
        } else {
            &self.filtered_bind_group
        })
    }

    pub(super) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn size(&self) -> u32 {
        self.size
    }

    /// One texel's share of the atlas side. The side is a power of two, so
    /// this reciprocal is exact and a coordinate times it is the coordinate
    /// divided by the side: a glyph's quad takes eight of them.
    pub(crate) fn texel(&self) -> f32 {
        (self.size() as f32).recip()
    }

    pub(super) fn entry(&mut self, key: &GlyphAtlasSlotKey) -> Option<GlyphAtlasEntry> {
        self.entries.get(key).copied()
    }

    fn allocate(&mut self, width: u32, height: u32) -> Option<GlyphAtlasEntry> {
        let max_extent = self.size.saturating_sub(TEXT_GLYPH_ATLAS_PADDING * 2);
        if width == 0 || height == 0 || width > max_extent || height > max_extent {
            return None;
        }

        if self.cursor_x + width + TEXT_GLYPH_ATLAS_PADDING > self.size {
            self.cursor_x = TEXT_GLYPH_ATLAS_PADDING;
            self.cursor_y = self
                .cursor_y
                .saturating_add(self.row_height)
                .saturating_add(TEXT_GLYPH_ATLAS_PADDING);
            self.row_height = 0;
        }
        if self.cursor_y > self.size - height - TEXT_GLYPH_ATLAS_PADDING {
            return None;
        }

        let entry = GlyphAtlasEntry {
            x: self.cursor_x as u16,
            y: self.cursor_y as u16,
            width: width as u16,
            height: height as u16,
        };
        self.cursor_x = self
            .cursor_x
            .saturating_add(width)
            .saturating_add(TEXT_GLYPH_ATLAS_PADDING);
        self.row_height = self.row_height.max(height);
        Some(entry)
    }

    /// The atlas entry of `glyph`, placed now if the atlas does not hold it
    /// yet; a placed glyph reaches the texture at [`Self::flush_uploads`].
    /// `None` when the atlas is full.
    pub(super) fn upload_glyph(
        &mut self,
        (glyph, mask): (&AtlasGlyph, &GlyphMask),
        frame_stats: &mut gpu_stats::FrameStats,
    ) -> Option<GlyphAtlasEntry> {
        let key = GlyphAtlasSlotKey::new(glyph.key, glyph.color);
        if let Some(entry) = self.entry(&key) {
            frame_stats.record_text_glyph_atlas_hits(1);
            return Some(entry);
        }

        let width = u32::try_from(mask.width).ok()?;
        let height = u32::try_from(mask.height).ok()?;
        let entry = self.allocate(width, height)?;
        let correction = key.luminance.correction();
        self.pending_coverage.extend(
            mask.alpha
                .iter()
                .map(|&coverage| correction.apply(coverage)),
        );
        self.pending.push(entry);
        frame_stats.record_text_glyph_atlas_miss(width, height);
        self.entries.put(key, entry);
        Some(entry)
    }

    /// Writes the glyphs placed since the last call to the texture, one
    /// write for each run of them placed side by side on a row. The atlas
    /// never places a glyph where one was, so the run's padding and the
    /// rows under its shorter glyphs are empty, and the write leaves them
    /// so. A frame calls this before it submits draws that read them.
    pub(super) fn flush_uploads(
        &mut self,
        queue: &wgpu::Queue,
        frame_stats: &mut gpu_stats::FrameStats,
    ) {
        let mut coverage = 0;
        let mut run_start = 0;
        while run_start < self.pending.len() {
            let first = self.pending[run_start];
            let mut run_end = run_start + 1;
            while self
                .pending
                .get(run_end)
                .is_some_and(|next| next.y == first.y && next.x > self.pending[run_end - 1].x)
            {
                run_end += 1;
            }
            let run = &self.pending[run_start..run_end];
            let last = run[run.len() - 1];
            let x = u32::from(first.x);
            let width = u32::from(last.x) + u32::from(last.width) - x;
            let height = run
                .iter()
                .map(|entry| u32::from(entry.height))
                .max()
                .unwrap_or(0);
            self.upload_scratch.clear();
            self.upload_scratch.resize((width * height) as usize, 0);
            for entry in run {
                let glyph_width = usize::from(entry.width);
                let left = usize::from(entry.x) - x as usize;
                for row in 0..usize::from(entry.height) {
                    let source = coverage + row * glyph_width;
                    let target = row * width as usize + left;
                    self.upload_scratch[target..target + glyph_width]
                        .copy_from_slice(&self.pending_coverage[source..source + glyph_width]);
                }
                coverage += glyph_width * usize::from(entry.height);
            }
            let upload_stats = crate::frame_graph::upload_texture(
                queue,
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x,
                        y: u32::from(first.y),
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &self.upload_scratch,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            frame_stats.record_command_stats(upload_stats);
            run_start = run_end;
        }
        self.pending.clear();
        self.pending_coverage.clear();
    }
}

/// `entry`'s rect in an atlas whose texel is `texel` of its side, an
/// exact power of two: the products are the quotients by the side.
pub(super) fn glyph_atlas_uv_rect(entry: GlyphAtlasEntry, texel: f32) -> ImageUvRect {
    let min = [entry.x as f32 * texel, entry.y as f32 * texel];
    let max = [
        (entry.x + entry.width) as f32 * texel,
        (entry.y + entry.height) as f32 * texel,
    ];
    let center_min = [
        (entry.x as f32 + 0.5) * texel,
        (entry.y as f32 + 0.5) * texel,
    ];
    let center_max = [
        (entry.x as f32 + entry.width as f32 - 0.5).max(entry.x as f32 + 0.5) * texel,
        (entry.y as f32 + entry.height as f32 - 0.5).max(entry.y as f32 + 0.5) * texel,
    ];
    ImageUvRect {
        min,
        max,
        sample_bounds: [center_min[0], center_min[1], center_max[0], center_max[1]],
    }
}
