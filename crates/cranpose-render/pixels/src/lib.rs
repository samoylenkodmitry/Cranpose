#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod damage;
mod draw;
mod growth_trim;
mod layout_walk;
mod moved_subtrees;
mod pipeline;
mod pixel;
mod raster_scene;
mod rewrite;
/// The retained scene and hit regions of the pixel renderer.
pub mod scene;
mod scene_item;
pub mod sprites;
mod style;
mod surface;
#[cfg(test)]
#[path = "tests/text_draws.rs"]
mod text_draws;

pub use cranpose_render_common::rgb565::Rgb565Order;
use cranpose_render_common::{
    RenderScene, Renderer, SceneUpdates,
    rgb565::{Rgb565Be, Rgb565Le},
    scene_builder::build_graph_from_layout_tree,
    text_fonts::TextFonts,
};
use cranpose_ui::LayoutTree;
use cranpose_ui_graphics::{MemoryHeadroom, Size};
pub use draw::draw_graph;
use pixel::Pixel;
pub use scene::{HitRegion, Scene};

/// A rectangle of device pixels: columns `x..x + width` of rows
/// `y..y + height`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    /// The first column.
    pub x: u32,
    /// The first row.
    pub y: u32,
    /// The number of columns.
    pub width: u32,
    /// The number of rows.
    pub height: u32,
}

fn pixel_rect(bounds: draw::ClipBounds) -> PixelRect {
    PixelRect {
        x: bounds.min_x.max(0) as u32,
        y: bounds.min_y.max(0) as u32,
        width: (bounds.max_x - bounds.min_x).max(0) as u32,
        height: (bounds.max_y - bounds.min_y).max(0) as u32,
    }
}

fn clip_bounds(area: PixelRect) -> draw::ClipBounds {
    draw::ClipBounds {
        min_x: i32::try_from(area.x).unwrap_or(i32::MAX),
        min_y: i32::try_from(area.y).unwrap_or(i32::MAX),
        max_x: i32::try_from(area.x.saturating_add(area.width)).unwrap_or(i32::MAX),
        max_y: i32::try_from(area.y.saturating_add(area.height)).unwrap_or(i32::MAX),
    }
}

fn valid_scale(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

/// A renderer that draws the scene into a caller-owned RGBA pixel buffer on the CPU.
pub struct PixelsRenderer {
    scene: Scene,
    text: draw::TextRaster,
    /// The app context this renderer gave its text measurer.
    app_context: Option<std::rc::Weak<cranpose_ui::AppContext>>,
}

impl Default for PixelsRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl PixelsRenderer {
    /// Creates a renderer that draws text in the embedded default font when the
    /// `embedded-default-font` feature is on, and draws no text otherwise.
    pub fn new() -> Self {
        Self::with_fonts(&[])
    }

    /// A renderer that draws text in `fonts`. With none that parses, it uses
    /// the embedded default font when the `embedded-default-font` feature is
    /// on, and draws no text otherwise.
    pub fn with_fonts(fonts: &[&'static [u8]]) -> Self {
        Self {
            scene: Scene::new(),
            text: draw::TextRaster::new(TextFonts::from_fonts_or_default(fonts)),
            app_context: None,
        }
    }

    /// Draws the scene into `frame`, an RGBA8 buffer of `width` by `height`
    /// pixels, at the scale the renderer builds it at (see
    /// [`Renderer::set_root_scale`]).
    pub fn draw(&self, frame: &mut [u8], width: u32, height: u32) {
        draw::draw_raster_frame(
            frame,
            (width, height),
            self.scene.raster(),
            self.scene.diagnostics(),
            &self.text,
        );
    }

    /// Puts into `changed` the areas of the display, `width` by `height`
    /// device pixels, whose pixels differ from the picture the host took
    /// before: the whole display the first time and after a change of size
    /// or scale. The host draws them with [`Self::draw_picture_area`].
    pub fn update_picture(&mut self, width: u32, height: u32, changed: &mut Vec<PixelRect>) {
        let frame = clip_bounds(PixelRect {
            x: 0,
            y: 0,
            width,
            height,
        });
        let mut areas = Vec::new();
        self.scene.take_changes(frame, &mut areas);
        changed.clear();
        changed.extend(areas.into_iter().map(pixel_rect));
        // With little memory left, the spill an update took for a subtree
        // that pushed other items than before goes, and so does the room the
        // scene's lists grew past their items.
        if cranpose_ui::current_memory_headroom() == MemoryHeadroom::Tight {
            self.scene.trim();
        }
    }

    /// Rasterizes the glyphs of the text in `area` of the picture into the
    /// renderer's glyph cache, so that drawing the area later takes them from
    /// it. A host calls it while it waits anyway, such as for a display that
    /// is still starting up.
    pub fn prepare_picture_area(&self, area: PixelRect) {
        if let Some(raster) = self.scene.raster() {
            draw::prepare_raster_window(raster, clip_bounds(area), &self.text);
        }
    }

    /// Draws `area` of the picture into `pixels` as straight-alpha RGBA8 rows
    /// of `area.width` pixels, `area.height` of them. Draws nothing before
    /// the first scene build or when `pixels` is shorter than the area.
    pub fn draw_picture_area(&self, area: PixelRect, pixels: &mut [u8]) {
        self.draw_picture_area_as(area, pixels.as_chunks_mut::<4>().0);
    }

    /// Draws `area` of the picture into `pixels` as rows of 16-bit pixels, 5
    /// bits of red, 6 of green and 5 of blue, stored in `order`, as a display
    /// takes them: `area.width` pixels a row, `area.height` rows. Each pixel
    /// keeps the high bits of the bytes [`Self::draw_picture_area`] draws,
    /// except where a drawing blends over another, which blends over the
    /// 16-bit pixel.
    pub fn draw_picture_area_rgb565(
        &self,
        area: PixelRect,
        pixels: &mut [u16],
        order: Rgb565Order,
    ) {
        match order {
            Rgb565Order::LittleEndian => {
                self.draw_picture_area_as::<Rgb565Le>(area, bytemuck::cast_slice_mut(pixels));
            }
            Rgb565Order::BigEndian => {
                self.draw_picture_area_as::<Rgb565Be>(area, bytemuck::cast_slice_mut(pixels));
            }
        }
    }

    // Out of line, one function a format: a display takes one byte order, and
    // a build that keeps its drawing in fast memory keeps only that one.
    #[inline(never)]
    fn draw_picture_area_as<P: Pixel>(&self, area: PixelRect, pixels: &mut [P]) {
        let Some(raster) = self.scene.raster() else {
            return;
        };
        let len = area.width as usize * area.height as usize;
        let Some(pixels) = pixels.get_mut(..len) else {
            return;
        };
        let mut target = draw::Target {
            pixels,
            left: i32::try_from(area.x).unwrap_or(i32::MAX),
            top: i32::try_from(area.y).unwrap_or(i32::MAX),
            width: area.width,
            height: area.height,
        };
        draw::draw_raster_window(
            &mut target,
            raster,
            self.scene.reach(),
            self.scene.diagnostics(),
            &self.text,
        );
    }

    /// The scene as sprites for a GPU compositor of a `width` by `height`
    /// display, into `frame`, which keeps its textures from the frames
    /// before.
    pub fn compose_sprites(&self, frame: &mut sprites::SpriteFrame, width: u32, height: u32) {
        if let Some(raster) = self.scene.raster() {
            frame.compose(raster, width, height, self.scene.diagnostics(), &self.text);
        }
    }

    fn attach_app_context_services(&mut self, app_context: &cranpose_ui::AppContext) {
        app_context.set_text_fonts(self.text.fonts.resolver());
        self.app_context = Some(app_context.downgrade());
    }
}

impl Renderer for PixelsRenderer {
    type Scene = Scene;
    type Error = std::convert::Infallible;

    fn attach_app_context_services(&mut self, app_context: &cranpose_ui::AppContext) {
        Self::attach_app_context_services(self, app_context);
    }

    #[cfg(feature = "installed-fonts")]
    fn apply_installed_fonts(&mut self, app_context: &cranpose_ui::AppContext) {
        let Some(fonts) = self.text.fonts.with_installed_faces() else {
            return;
        };
        self.text = draw::TextRaster::new(fonts);
        let attached = self
            .app_context
            .as_ref()
            .and_then(std::rc::Weak::upgrade)
            .is_some_and(|attached| std::ptr::eq(&*attached, app_context));
        if attached {
            Self::attach_app_context_services(self, app_context);
        }
    }

    fn set_root_scale(&mut self, scale: f32) {
        self.scene.set_scale(valid_scale(scale));
    }

    fn scene(&self) -> &Self::Scene {
        &self.scene
    }

    fn scene_mut(&mut self) -> &mut Self::Scene {
        &mut self.scene
    }

    fn rebuild_scene(
        &mut self,
        layout_tree: &LayoutTree,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.scene.clear();
        let graph = build_graph_from_layout_tree(layout_tree.root(), 1.0);
        self.scene.rebuild_from_graph(&graph, &self.text);
        Ok(())
    }

    fn rebuild_scene_from_applier(
        &mut self,
        applier: &mut cranpose_core::MemoryApplier,
        root: cranpose_core::NodeId,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.scene.rebuild(applier, root, &self.text);
        Ok(())
    }

    fn update_scene_from_applier(
        &mut self,
        applier: &mut cranpose_core::MemoryApplier,
        root: cranpose_core::NodeId,
        _viewport: Size,
        updates: SceneUpdates<'_>,
    ) -> Result<(), Self::Error> {
        self.scene.update(applier, root, updates, &self.text);
        Ok(())
    }
}
