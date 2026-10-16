#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod damage;
mod draw;
mod pipeline;
/// The retained scene and hit regions of the pixel renderer.
pub mod scene;
pub mod sprites;
mod style;
mod surface;

use cranpose_render_common::{
    RenderScene, Renderer, SceneUpdates,
    text_measure::{CachedFontTextMeasurer, SoftwareTextResources},
};
use cranpose_ui::LayoutTree;
use cranpose_ui_graphics::Size;
pub use draw::draw_scene;
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

/// The picture a host drew last. [`PixelsRenderer::update_picture`] moves it
/// to the current scene and finds the pixels that change, and
/// [`PixelsRenderer::draw_picture_area`] draws any part of it.
#[derive(Default)]
pub struct Picture {
    raster: Option<scene::RasterScene>,
    /// The scene before `raster`, whose storage the next one reuses.
    spare: scene::RasterScene,
    width: u32,
    height: u32,
    scale: f32,
    changed: Vec<draw::ClipBounds>,
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
            text: draw::TextRaster::new(SoftwareTextResources::from_fonts(fonts)),
            app_context: None,
        }
    }

    /// Draws the scene into `frame`, an RGBA8 buffer of `width` by `height` pixels, at one device
    /// pixel per logical pixel.
    pub fn draw(&self, frame: &mut [u8], width: u32, height: u32) {
        draw::draw_scene_scaled(frame, width, height, &self.scene, &self.text, 1.0);
    }

    /// Rasterizes logical scene coordinates at `scale` device pixels per point.
    /// Hit regions stay in logical coordinates. Invalid scales use 1.0.
    pub fn draw_scaled(&self, frame: &mut [u8], width: u32, height: u32, scale: f32) {
        draw::draw_scene_scaled(
            frame,
            width,
            height,
            &self.scene,
            &self.text,
            valid_scale(scale),
        );
    }

    /// Fills `raster` with the scene at `scale`, keeping its storage, and
    /// copying from `previous`, the frame before's, what did not change.
    fn fill_raster_scene(
        &self,
        raster: &mut scene::RasterScene,
        scale: f32,
        previous: Option<&scene::RasterScene>,
    ) {
        match self.scene.graph.as_ref() {
            Some(graph) => pipeline::fill_raster_scene(
                raster,
                graph,
                self.scene.diagnostics(),
                scale,
                previous,
                &self.text,
            ),
            None => raster.clear(),
        }
    }

    /// Moves `picture` to the current scene, drawn `width` by `height` device
    /// pixels at `scale` device pixels per point, and puts into `changed` the
    /// areas whose pixels differ from the picture it held: the whole frame the
    /// first time and after a change of size or scale. Invalid scales use 1.0.
    pub fn update_picture(
        &self,
        picture: &mut Picture,
        width: u32,
        height: u32,
        scale: f32,
        changed: &mut Vec<PixelRect>,
    ) {
        let scale = valid_scale(scale);
        let mut raster = std::mem::take(&mut picture.spare);
        self.fill_raster_scene(&mut raster, scale, picture.raster.as_ref());
        changed.clear();
        match picture.raster.as_ref() {
            Some(old)
                if (picture.width, picture.height, picture.scale) == (width, height, scale) =>
            {
                let window = draw::ClipBounds {
                    min_x: 0,
                    min_y: 0,
                    max_x: i32::try_from(width).unwrap_or(i32::MAX),
                    max_y: i32::try_from(height).unwrap_or(i32::MAX),
                };
                damage::changed_areas(old, &raster, window, &mut picture.changed);
                changed.extend(picture.changed.iter().copied().map(pixel_rect));
            }
            _ if width > 0 && height > 0 => changed.push(PixelRect {
                x: 0,
                y: 0,
                width,
                height,
            }),
            _ => {}
        }
        if let Some(old) = picture.raster.replace(raster) {
            picture.spare = old;
        }
        picture.width = width;
        picture.height = height;
        picture.scale = scale;
    }

    /// Draws `area` of `picture` into `pixels` as straight-alpha RGBA8 rows of
    /// `area.width` pixels, `area.height` of them. Draws nothing before the
    /// first [`Self::update_picture`] or when `pixels` is shorter than the area.
    pub fn draw_picture_area(&self, picture: &Picture, area: PixelRect, pixels: &mut [u8]) {
        let Some(raster) = picture.raster.as_ref() else {
            return;
        };
        let len = area.width as usize * area.height as usize * 4;
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
        draw::draw_raster_window(&mut target, raster, self.scene.diagnostics(), &self.text);
    }
}

impl PixelsRenderer {
    /// The scene at `scale` device pixels per point as sprites for a GPU
    /// compositor, into `frame`, which keeps its textures from the frames
    /// before. Invalid scales use 1.0.
    pub fn compose_sprites(
        &self,
        frame: &mut sprites::SpriteFrame,
        width: u32,
        height: u32,
        scale: f32,
    ) {
        let mut scene = std::mem::take(&mut frame.scene);
        self.fill_raster_scene(&mut scene, valid_scale(scale), None);
        frame.compose(&scene, width, height, self.scene.diagnostics(), &self.text);
        frame.scene = scene;
    }
}

impl Renderer for PixelsRenderer {
    type Scene = Scene;
    type Error = std::convert::Infallible;

    fn attach_app_context_services(&mut self, app_context: &cranpose_ui::AppContext) {
        app_context.set_text_measurer(CachedFontTextMeasurer::with_text_resources(
            self.text.resources.clone(),
            64,
        ));
        self.app_context = Some(app_context.downgrade());
    }

    #[cfg(feature = "installed-fonts")]
    fn apply_installed_fonts(&mut self, app_context: &cranpose_ui::AppContext) {
        let Some(resources) = self.text.resources.with_installed_faces() else {
            return;
        };
        self.text = draw::TextRaster::new(resources);
        let attached = self
            .app_context
            .as_ref()
            .and_then(std::rc::Weak::upgrade)
            .is_some_and(|attached| std::ptr::eq(&*attached, app_context));
        if attached {
            self.attach_app_context_services(app_context);
        }
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
        pipeline::render_layout_tree(layout_tree.root(), &mut self.scene);
        Ok(())
    }

    fn rebuild_scene_from_applier(
        &mut self,
        applier: &mut cranpose_core::MemoryApplier,
        root: cranpose_core::NodeId,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.scene.rebuild_from_applier(applier, root);
        Ok(())
    }

    fn update_scene_from_applier(
        &mut self,
        applier: &mut cranpose_core::MemoryApplier,
        root: cranpose_core::NodeId,
        _viewport: Size,
        updates: SceneUpdates<'_>,
    ) -> Result<(), Self::Error> {
        self.scene.update_from_applier(applier, root, updates, true);
        Ok(())
    }

    fn update_visual_scene_from_applier(
        &mut self,
        applier: &mut cranpose_core::MemoryApplier,
        root: cranpose_core::NodeId,
        _viewport: Size,
        updates: SceneUpdates<'_>,
    ) -> Result<(), Self::Error> {
        self.scene
            .update_from_applier(applier, root, updates, false);
        Ok(())
    }
}
