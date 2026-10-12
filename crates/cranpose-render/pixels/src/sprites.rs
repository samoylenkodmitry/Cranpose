//! The scene as sprites for a GPU compositor other than wgpu: SpriteKit on
//! watchOS, which gives an app no Metal of its own.
//!
//! Every item becomes one quad in device pixels, drawn in order, cut to the
//! whole pixels its clip leaves, as the rasterizer cuts it. The shapes the
//! compositor's shader covers -- filled and stroked rounded rects, arcs and
//! lines, painted with a solid colour or a two-colour gradient -- travel as
//! numbers. Everything else is drawn here, once, by the rasterizer into a
//! texture of its own, which is kept while the item stays the same: moving it
//! by whole pixels moves the quad and keeps the texture.
//!
//! A sprite is [`SPRITE_FLOATS`] numbers; [`field`] names where each part
//! starts. Positions are device pixels with the origin at the top left.

use std::{
    collections::HashMap,
    fmt::Write as _,
    hash::{DefaultHasher, Hasher},
};

use cranpose_render_common::{
    brush_sampling::color_to_rgba, graph_scene::RenderDiagnostics,
    software_text_raster::rasterize_annotated_text_region, text_measure::SoftwareTextResources,
};
use cranpose_ui::text::TextMotion;
use cranpose_ui_graphics::{BlendMode, Brush, Color, Point, Rect, StrokeCap, StrokeJoin, TileMode};

use crate::{
    draw::{
        ClipBounds, RenderItem, ShapeCoverage, Target, clip_rect_to_window, draw_item, place_shape,
        snap_delta_for_anchor,
    },
    scene::{DrawShape, RasterScene},
};

/// How many numbers make one sprite.
pub const SPRITE_FLOATS: usize = 36;

/// Where each part of a sprite starts, and what it holds.
pub mod field {
    /// The quad: x, y, width, height.
    pub const FRAME: usize = 0;
    /// What the quad holds ([`super::SpriteKind`]), the texture it shows
    /// (0 for none), how it blends (0 source over, 1 destination out) and its
    /// brush (0 solid, 1 linear, 2 radial, 3 sweep).
    pub const KIND: usize = 4;
    /// The geometry, by kind. A texture: the rect the whole texture covers.
    /// A fill or a stroke: the rect. An arc: centre x and y, inner and outer
    /// radius. A line: start x and y, end x and y.
    pub const GEOMETRY: usize = 8;
    /// A fill or a stroke: the corner radii, top-left, top-right,
    /// bottom-right, bottom-left. An arc: start angle, sweep, cap (0 butt,
    /// 1 round, 2 square). A line: half width, cap.
    pub const DETAIL: usize = 12;
    /// A fill: whether its corners are rounded (1) or not (0). A stroke: half
    /// width and join (0 miter, 1 round, 2 bevel). Third: a gradient's tile
    /// mode (0 clamp, 1 repeated, 2 mirror, 3 decal).
    pub const STROKE: usize = 16;
    /// The brush's first colour, straight RGBA.
    pub const COLOR: usize = 20;
    /// A gradient's second colour, straight RGBA.
    pub const COLOR_END: usize = 24;
    /// A gradient's geometry: linear start x, y and end x, y; radial centre
    /// x, y and radius; sweep centre x, y.
    pub const GRADIENT: usize = 28;
    /// A gradient's two stops, then the origin its dither is measured from.
    pub const STOPS: usize = 32;
}

/// What one sprite holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SpriteKind {
    Texture = 0,
    Fill = 1,
    Stroke = 2,
    Arc = 3,
    Line = 4,
}

/// A texture the compositor has not seen yet: premultiplied RGBA8, row by
/// row from the top.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureUpload {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// One frame of sprites, and the textures they use. It keeps its textures
/// from frame to frame: compose into the same one every frame.
#[derive(Default)]
pub struct SpriteFrame {
    sprites: Vec<f32>,
    uploads: Vec<TextureUpload>,
    releases: Vec<u32>,
    textures: HashMap<u64, CachedTexture>,
    next_texture: u32,
    generation: u64,
}

struct CachedTexture {
    id: u32,
    /// Where the texture sits for the item at its canonical place.
    placement: Rect,
    last_used: u64,
}

/// Frames a texture outlives the last item that showed it, so an item that
/// blinks does not draw itself again each time it comes back.
const TEXTURE_RETENTION_FRAMES: u64 = 60;

impl SpriteFrame {
    /// The sprites, [`SPRITE_FLOATS`] numbers each, in drawing order.
    pub fn sprites(&self) -> &[f32] {
        &self.sprites
    }

    pub fn sprite_count(&self) -> usize {
        self.sprites.len() / SPRITE_FLOATS
    }

    /// Textures first used this frame.
    pub fn uploads(&self) -> &[TextureUpload] {
        &self.uploads
    }

    /// Textures no sprite has used for a while; the compositor drops them.
    pub fn releases(&self) -> &[u32] {
        &self.releases
    }

    /// Keeps the sprites and forgets the uploads and releases already handed
    /// over, for a frame whose picture did not change.
    pub fn settle(&mut self) {
        self.uploads.clear();
        self.releases.clear();
    }

    pub(crate) fn compose(
        &mut self,
        scene: &RasterScene,
        width: u32,
        height: u32,
        diagnostics: &RenderDiagnostics,
        text_resources: &SoftwareTextResources,
    ) {
        self.sprites.clear();
        self.uploads.clear();
        self.releases.clear();
        self.generation += 1;
        let screen = ClipBounds {
            min_x: 0,
            min_y: 0,
            max_x: width as i32,
            max_y: height as i32,
        };
        for item in scene.ordered_items() {
            if let RenderItem::Shape(index) = item
                && let Some(sprite) = shape_sprite(&scene.shapes[index], screen)
            {
                self.sprites.extend_from_slice(&sprite);
                continue;
            }
            self.texture_sprite(scene, item, screen, diagnostics, text_resources);
        }
        let generation = self.generation;
        let releases = &mut self.releases;
        self.textures.retain(|_, texture| {
            let keep = generation - texture.last_used <= TEXTURE_RETENTION_FRAMES;
            if !keep {
                releases.push(texture.id);
            }
            keep
        });
    }

    /// Shows `item` from a texture the rasterizer drew it into, drawing it
    /// first if no texture holds it at its canonical place.
    fn texture_sprite(
        &mut self,
        scene: &RasterScene,
        item: RenderItem,
        screen: ClipBounds,
        diagnostics: &RenderDiagnostics,
        text_resources: &SoftwareTextResources,
    ) {
        let Some((canonical, offset, clip)) = Canonical::of(scene, item) else {
            return;
        };
        let key = canonical.key();
        let generation = self.generation;
        let (id, placement) = match self.textures.get_mut(&key) {
            Some(texture) => {
                texture.last_used = generation;
                (texture.id, texture.placement)
            }
            None => {
                let Some((placement, pixels)) =
                    canonical.rasterize(diagnostics, text_resources)
                else {
                    return;
                };
                self.next_texture = self.next_texture.wrapping_add(1).max(1);
                let id = self.next_texture;
                self.uploads.push(TextureUpload {
                    id,
                    width: placement.width as u32,
                    height: placement.height as u32,
                    pixels,
                });
                self.textures.insert(
                    key,
                    CachedTexture {
                        id,
                        placement,
                        last_used: generation,
                    },
                );
                (id, placement)
            }
        };
        let placement = placement.translate(offset.x, offset.y);
        let Some(frame) = clip_rect_to_window(placement, clip, screen) else {
            return;
        };
        let mut sprite = [0.0; SPRITE_FLOATS];
        put_frame(&mut sprite, frame);
        sprite[field::KIND] = SpriteKind::Texture as u8 as f32;
        sprite[field::KIND + 1] = id as f32;
        put_rect(&mut sprite, field::GEOMETRY, placement);
        self.sprites.extend_from_slice(&sprite);
    }
}

fn put_frame(sprite: &mut [f32; SPRITE_FLOATS], frame: ClipBounds) {
    sprite[field::FRAME] = frame.min_x as f32;
    sprite[field::FRAME + 1] = frame.min_y as f32;
    sprite[field::FRAME + 2] = (frame.max_x - frame.min_x) as f32;
    sprite[field::FRAME + 3] = (frame.max_y - frame.min_y) as f32;
}

fn put_rect(sprite: &mut [f32; SPRITE_FLOATS], at: usize, rect: Rect) {
    sprite[at..at + 4].copy_from_slice(&[rect.x, rect.y, rect.width, rect.height]);
}

fn put_color(sprite: &mut [f32; SPRITE_FLOATS], at: usize, color: Color) {
    sprite[at..at + 4].copy_from_slice(&color_to_rgba(color));
}

fn cap_code(cap: StrokeCap) -> f32 {
    match cap {
        StrokeCap::Butt => 0.0,
        StrokeCap::Round => 1.0,
        StrokeCap::Square => 2.0,
    }
}

fn join_code(join: StrokeJoin) -> f32 {
    match join {
        StrokeJoin::Miter => 0.0,
        StrokeJoin::Round => 1.0,
        StrokeJoin::Bevel => 2.0,
    }
}

fn tile_code(tile_mode: TileMode) -> f32 {
    match tile_mode {
        TileMode::Clamp => 0.0,
        TileMode::Repeated => 1.0,
        TileMode::Mirror => 2.0,
        TileMode::Decal => 3.0,
    }
}

/// The sprite the compositor's shader draws `draw` with, or `None` when the
/// shader does not cover it and a texture must.
fn shape_sprite(draw: &DrawShape, screen: ClipBounds) -> Option<[f32; SPRITE_FLOATS]> {
    let blend = match draw.blend_mode {
        BlendMode::DstOut => 1.0,
        _ => 0.0,
    };
    let placed = place_shape(draw)?;
    let mut sprite = [0.0; SPRITE_FLOATS];
    match placed.coverage {
        ShapeCoverage::Fill { rect, radii } => {
            sprite[field::KIND] = SpriteKind::Fill as u8 as f32;
            put_rect(&mut sprite, field::GEOMETRY, rect);
            if let Some(radii) = radii {
                sprite[field::DETAIL..field::DETAIL + 4].copy_from_slice(&[
                    radii.top_left,
                    radii.top_right,
                    radii.bottom_right,
                    radii.bottom_left,
                ]);
                sprite[field::STROKE] = 1.0;
            }
        }
        ShapeCoverage::Stroke {
            rect,
            radii,
            half_width,
            join,
        } => {
            sprite[field::KIND] = SpriteKind::Stroke as u8 as f32;
            put_rect(&mut sprite, field::GEOMETRY, rect);
            let radii = radii.unwrap_or_default();
            sprite[field::DETAIL..field::DETAIL + 4].copy_from_slice(&[
                radii.top_left,
                radii.top_right,
                radii.bottom_right,
                radii.bottom_left,
            ]);
            sprite[field::STROKE] = half_width;
            sprite[field::STROKE + 1] = join_code(join);
        }
        ShapeCoverage::Arc(arc) => {
            sprite[field::KIND] = SpriteKind::Arc as u8 as f32;
            sprite[field::GEOMETRY..field::GEOMETRY + 4].copy_from_slice(&[
                arc.center.x,
                arc.center.y,
                arc.inner_radius,
                arc.outer_radius,
            ]);
            sprite[field::DETAIL..field::DETAIL + 3].copy_from_slice(&[
                arc.start_angle,
                arc.sweep_angle,
                cap_code(arc.cap),
            ]);
        }
        ShapeCoverage::Line(line) => {
            sprite[field::KIND] = SpriteKind::Line as u8 as f32;
            sprite[field::GEOMETRY..field::GEOMETRY + 4].copy_from_slice(&[
                line.start.x,
                line.start.y,
                line.end.x,
                line.end.y,
            ]);
            sprite[field::DETAIL] = line.half_width;
            sprite[field::DETAIL + 1] = cap_code(line.cap);
        }
        ShapeCoverage::Trapezoid(_) => return None,
    }
    put_brush(&mut sprite, &draw.brush, placed.rect)?;
    sprite[field::KIND + 2] = blend;
    let frame = clip_rect_to_window(placed.coverage.reach(placed.rect), draw.clip, screen)?;
    put_frame(&mut sprite, frame);
    // The dither's origin, which the shader measures pixels from.
    sprite[field::STOPS + 2] = placed.dither_origin.x;
    sprite[field::STOPS + 3] = placed.dither_origin.y;
    Some(sprite)
}

/// Writes the brush the shader paints with, resolved against `rect` as the
/// rasterizer resolves it; `None` for a gradient of more than two colours.
fn put_brush(sprite: &mut [f32; SPRITE_FLOATS], brush: &Brush, rect: Rect) -> Option<()> {
    let two_colours = |colors: &[Color], stops: Option<&[f32]>| match (colors, stops) {
        ([only], _) => Some((*only, *only, [0.0, 1.0])),
        ([start, end], Some([from, to])) => Some((*start, *end, [*from, *to])),
        ([start, end], _) => Some((*start, *end, [0.0, 1.0])),
        _ => None,
    };
    let resolve = |origin: f32, extent: f32, value: f32| {
        if value.is_finite() {
            origin + value
        } else if value.is_sign_positive() {
            origin + extent
        } else {
            origin
        }
    };
    let (code, (start, end, stops), gradient, tile) = match brush {
        Brush::Solid(color) => (0.0, (*color, *color, [0.0, 1.0]), [0.0; 4], TileMode::Clamp),
        Brush::LinearGradient {
            colors,
            stops,
            start,
            end,
            tile_mode,
        } => (
            1.0,
            two_colours(colors, stops.as_deref())?,
            [
                resolve(rect.x, rect.width, start.x),
                resolve(rect.y, rect.height, start.y),
                resolve(rect.x, rect.width, end.x),
                resolve(rect.y, rect.height, end.y),
            ],
            *tile_mode,
        ),
        Brush::RadialGradient {
            colors,
            stops,
            center,
            radius,
            tile_mode,
        } => (
            2.0,
            two_colours(colors, stops.as_deref())?,
            [
                rect.x + center.x,
                rect.y + center.y,
                radius.max(f32::EPSILON),
                0.0,
            ],
            *tile_mode,
        ),
        Brush::SweepGradient {
            colors,
            stops,
            center,
        } => (
            3.0,
            two_colours(colors, stops.as_deref())?,
            [rect.x + center.x, rect.y + center.y, 0.0, 0.0],
            TileMode::Clamp,
        ),
    };
    sprite[field::KIND + 3] = code;
    put_color(sprite, field::COLOR, start);
    put_color(sprite, field::COLOR_END, end);
    sprite[field::GRADIENT..field::GRADIENT + 4].copy_from_slice(&gradient);
    sprite[field::STOPS] = stops[0];
    sprite[field::STOPS + 1] = stops[1];
    sprite[field::STROKE + 2] = tile_code(tile);
    Some(())
}

/// An item moved to its canonical place: shifted by whole pixels so its own
/// rect starts within the first pixel, with no clip. Two frames that show the
/// same item at whole-pixel distances share one canonical item.
struct Canonical {
    scene: RasterScene,
    item: RenderItem,
}

impl Canonical {
    /// The canonical item, the whole-pixel shift back to where `item` is,
    /// and its clip.
    fn of(scene: &RasterScene, item: RenderItem) -> Option<(Self, Point, Option<Rect>)> {
        let mut canonical = RasterScene::new();
        let (origin, clip) = match item {
            RenderItem::Shape(index) => {
                let draw = &scene.shapes[index];
                (Point::new(draw.rect.x, draw.rect.y), draw.clip)
            }
            RenderItem::Image(index) => {
                let draw = &scene.images[index];
                (Point::new(draw.rect.x, draw.rect.y), draw.clip)
            }
            RenderItem::Text(index) => {
                let draw = &scene.texts[index];
                (Point::new(draw.rect.x, draw.rect.y), draw.clip)
            }
            RenderItem::Shadow(index) => {
                let draw = &scene.shadows[index];
                let snap = draw.snap_anchor.map(snap_delta_for_anchor).unwrap_or_default();
                (
                    Point::new(draw.shadow.bounds.x, draw.shadow.bounds.y),
                    draw.clip.map(|clip| clip.translate(snap.x, snap.y)),
                )
            }
        };
        if !(origin.x.is_finite() && origin.y.is_finite()) {
            return None;
        }
        let shift = Point::new(origin.x.floor(), origin.y.floor());
        let item = match item {
            RenderItem::Shape(index) => {
                canonical
                    .shapes
                    .push(scene.shapes[index].translated(-shift.x, -shift.y));
                RenderItem::Shape(0)
            }
            RenderItem::Image(index) => {
                canonical
                    .images
                    .push(scene.images[index].translated(-shift.x, -shift.y));
                RenderItem::Image(0)
            }
            RenderItem::Text(index) => {
                canonical
                    .texts
                    .push(scene.texts[index].translated(-shift.x, -shift.y));
                RenderItem::Text(0)
            }
            RenderItem::Shadow(index) => {
                canonical
                    .shadows
                    .push(scene.shadows[index].translated(-shift.x, -shift.y));
                RenderItem::Shadow(0)
            }
        };
        Some((
            Self {
                scene: canonical,
                item,
            },
            shift,
            clip,
        ))
    }

    /// A hash of everything that decides the canonical item's pixels.
    fn key(&self) -> u64 {
        let Self { scene, item } = self;
        let mut hasher = HashWriter(DefaultHasher::new());
        // The `Debug` forms name every field, positions included; an image's
        // pixels are named by the bitmap's id instead of printed.
        let _ = match item {
            RenderItem::Shape(_) => write!(hasher, "shape {:?}", scene.shapes[0]),
            RenderItem::Text(_) => write!(hasher, "text {:?}", scene.texts[0]),
            RenderItem::Shadow(_) => write!(hasher, "shadow {:?}", scene.shadows[0]),
            RenderItem::Image(_) => {
                let draw = &scene.images[0];
                write!(
                    hasher,
                    "image {} {:?} {:?} {} {:?} {:?} {:?} {:?}",
                    draw.image.id(),
                    draw.rect,
                    draw.snap_anchor,
                    draw.alpha,
                    draw.color_filter,
                    draw.sampling,
                    draw.blend_mode,
                    draw.src_rect,
                )
            }
        };
        hasher.0.finish()
    }

    /// Draws the canonical item into a texture of the whole pixels it
    /// reaches: where the texture sits and its premultiplied pixels.
    fn rasterize(
        &self,
        diagnostics: &RenderDiagnostics,
        text_resources: &SoftwareTextResources,
    ) -> Option<(Rect, Vec<u8>)> {
        let Self { scene, item } = self;
        if let RenderItem::Text(_) = item {
            return rasterize_text(&scene.texts[0], text_resources);
        }
        let reach = scene.item_reach(*item)?;
        let window = clip_rect_to_window(
            reach,
            None,
            ClipBounds {
                min_x: i32::MIN / 4,
                min_y: i32::MIN / 4,
                max_x: i32::MAX / 4,
                max_y: i32::MAX / 4,
            },
        )?;
        let width = (window.max_x - window.min_x) as u32;
        let height = (window.max_y - window.min_y) as u32;
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        let mut target = Target {
            pixels: &mut pixels,
            left: window.min_x,
            top: window.min_y,
            width,
            height,
        };
        draw_item(&mut target, scene, *item, diagnostics, text_resources);
        Some((
            Rect {
                x: window.min_x as f32,
                y: window.min_y as f32,
                width: width as f32,
                height: height as f32,
            },
            pixels,
        ))
    }
}

/// Text drawn as the rasterizer draws it, into its own image.
fn rasterize_text(
    draw: &crate::scene::TextDraw,
    text_resources: &SoftwareTextResources,
) -> Option<(Rect, Vec<u8>)> {
    let scale = draw.scale.max(0.0);
    if scale == 0.0 {
        return None;
    }
    let static_motion = draw
        .text_style
        .paragraph_style
        .text_motion
        .unwrap_or(TextMotion::Static)
        == TextMotion::Static;
    let snap = if static_motion {
        draw.snap_anchor.map(snap_delta_for_anchor).unwrap_or_default()
    } else {
        Point::default()
    };
    let rect = draw.rect.translate(snap.x, snap.y);
    let raster_rect = if static_motion {
        Rect {
            x: rect.x.round(),
            y: rect.y.round(),
            width: if rect.width > 0.0 {
                rect.width.ceil().max(1.0)
            } else {
                rect.width
            },
            height: if rect.height > 0.0 {
                rect.height.ceil().max(1.0)
            } else {
                rect.height
            },
        }
    } else {
        rect
    };
    let image = rasterize_annotated_text_region(
        draw.text.as_ref(),
        raster_rect,
        Point::new(raster_rect.x, raster_rect.y),
        &draw.text_style,
        draw.color,
        draw.font_size,
        scale,
        text_resources.fonts(),
        None,
    )?;
    let origin = if static_motion {
        Point::new(raster_rect.x, raster_rect.y)
    } else {
        Point::new(rect.x, rect.y)
    };
    let mut pixels = image.rgba8_pixels().to_vec();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let alpha = u32::from(pixel[3]);
        for channel in &mut pixel[..3] {
            *channel = ((u32::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
    Some((
        Rect {
            x: origin.x,
            y: origin.y,
            width: image.width() as f32,
            height: image.height() as f32,
        },
        pixels,
    ))
}

struct HashWriter(DefaultHasher);

impl std::fmt::Write for HashWriter {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.0.write(text.as_bytes());
        Ok(())
    }
}
