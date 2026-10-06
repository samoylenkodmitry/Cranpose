//! Elevation shadows drawn straight into a pass. Each pass of a layer's
//! shadow is Skia's round rect shadow, which the fragment stage evaluates at
//! every pixel it covers, so a shadow takes no surface, blur or composite.

use bytemuck::{Pod, Zeroable};
use cranpose_render_common::layer_shadow::ShadowRRect;
use cranpose_ui_graphics::{BlendMode, Rect};

use crate::{
    geometry::{canonicalize_device_coordinate, snap_delta_for_anchor},
    render::{blend_state_for_mode, create_render_pipeline_logged, overlay_depth_state},
    scene::RRectShadowDraw,
    shared_shader::SharedShader,
};

/// Four corners per instance, drawn as a triangle strip.
pub(crate) const SHADOW_QUAD_CORNERS: u32 = 4;

/// One rect of a shadow pass the pipeline rasterizes, with the whole
/// shadow's parameters. See `rrect_shadow.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct ShadowInstance {
    draw: [f32; 4],
    bounds: [f32; 4],
    radii: [f32; 4],
    ramp: [f32; 4],
    color: [f32; 4],
}

impl ShadowInstance {
    const ATTRIBS: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
        0 => Float32x4,
        1 => Float32x4,
        2 => Float32x4,
        3 => Float32x4,
        4 => Float32x4,
    ];

    pub(crate) fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &Self::ATTRIBS,
        }
    }
}

/// `draw`'s shadow in device pixels: moved by its anchor's snap, then scaled.
pub(crate) fn device_shadow(draw: &RRectShadowDraw, root_scale: f32) -> ShadowRRect {
    let snap = draw
        .snap_anchor
        .map(|anchor| snap_delta_for_anchor(anchor, root_scale))
        .unwrap_or_default();
    draw.shadow.translated(snap.x, snap.y).scaled(root_scale)
}

/// The logical rect `draw` may cover: its bounds moved by its anchor's snap,
/// within its clip.
pub(crate) fn rrect_shadow_bounds(draw: &RRectShadowDraw, root_scale: f32) -> Option<Rect> {
    let snap = draw
        .snap_anchor
        .map(|anchor| snap_delta_for_anchor(anchor, root_scale))
        .unwrap_or_default();
    let bounds = draw.shadow.bounds.translate(snap.x, snap.y);
    match draw.clip {
        Some(clip) => clip.translate(snap.x, snap.y).intersect(bounds),
        None => Some(bounds),
    }
}

/// The device rect `draw`'s clip lets it cover: the clip moved and scaled
/// as the shadow is, its edges pushed out to whole pixels as a scissor's.
pub(crate) fn device_clip(draw: &RRectShadowDraw, root_scale: f32) -> Option<[f32; 4]> {
    let snap = draw
        .snap_anchor
        .map(|anchor| snap_delta_for_anchor(anchor, root_scale))
        .unwrap_or_default();
    draw.clip.map(|clip| {
        let clip = clip.translate(snap.x, snap.y);
        let edge = |value: f32| canonicalize_device_coordinate(value * root_scale);
        [
            edge(clip.x).floor(),
            edge(clip.y).floor(),
            edge(clip.x + clip.width).ceil(),
            edge(clip.y + clip.height).ceil(),
        ]
    })
}

fn edges(rect: Rect) -> [f32; 4] {
    [rect.x, rect.y, rect.x + rect.width, rect.y + rect.height]
}

fn intersect(a: [f32; 4], b: [f32; 4]) -> Option<[f32; 4]> {
    let cut = [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ];
    (cut[0] < cut[2] && cut[1] < cut[3]).then_some(cut)
}

/// Appends the instances `draw`'s shadow takes in device pixels: the rects
/// around a square hole, which leave it out whole, or one rect whose pixels
/// test a round hole, each cut to the clip.
pub(crate) fn append_shadow_instances(
    draw: &RRectShadowDraw,
    root_scale: f32,
    instances: &mut Vec<ShadowInstance>,
) {
    let shadow = device_shadow(draw, root_scale);
    let bounds = edges(shadow.bounds);
    let Some(area) =
        device_clip(draw, root_scale).map_or(Some(bounds), |clip| intersect(bounds, clip))
    else {
        return;
    };
    let color = [
        draw.color.r(),
        draw.color.g(),
        draw.color.b(),
        draw.color.a(),
    ];
    let ramp = |hole_inset: f32, hole_radius: f32| {
        [
            shadow.umbra_inset,
            shadow.distance_correction,
            hole_inset,
            hole_radius,
        ]
    };
    let instance = |rect: [f32; 4], ramp: [f32; 4]| ShadowInstance {
        draw: rect,
        bounds,
        radii: shadow.radii,
        ramp,
        color,
    };
    match (shadow.hole, shadow.hole_rect()) {
        (Some(hole), Some(rect)) if hole.radius <= 0.0 => {
            let [left, top, right, bottom] = edges(rect);
            let bands = [
                [bounds[0], bounds[1], bounds[2], top],
                [bounds[0], bottom, bounds[2], bounds[3]],
                [bounds[0], top, left, bottom],
                [right, top, bounds[2], bottom],
            ];
            instances.extend(
                bands
                    .into_iter()
                    .filter_map(|band| intersect(band, area))
                    .map(|band| instance(band, ramp(0.0, -1.0))),
            );
        }
        (Some(hole), _) => instances.push(instance(area, ramp(hole.inset, hole.radius))),
        (None, _) => instances.push(instance(area, ramp(0.0, -1.0))),
    }
}

/// The round rect shadow pipeline for a pass with a depth buffer or not.
pub(crate) fn create_rrect_shadow_pipeline(
    device: &wgpu::Device,
    cache: Option<&wgpu::PipelineCache>,
    surface_format: wgpu::TextureFormat,
    shader: &SharedShader,
    depth: bool,
) -> wgpu::RenderPipeline {
    let module = shader.module();
    create_render_pipeline_logged(
        device,
        cache,
        if depth {
            "rrect-shadow depth"
        } else {
            "rrect-shadow"
        },
        wgpu::RenderPipelineDescriptor {
            label: Some("Round Rect Shadow Pipeline"),
            layout: Some(shader.layout()),
            vertex: wgpu::VertexState {
                module,
                entry_point: Some("rrect_shadow_vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(ShadowInstance::desc())],
            },
            fragment: Some(wgpu::FragmentState {
                module,
                entry_point: Some("rrect_shadow_fs_main"),
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
            depth_stencil: overlay_depth_state(depth),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        },
    )
}
