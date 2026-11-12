#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod damage;
mod draw;
mod growth_trim;
mod pipeline;
mod pixel;
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

use cranpose_core::{RecordRef, SceneUpdate, StoreId};
pub use cranpose_render_common::rgb565::Rgb565Order;
use cranpose_render_common::{
    Renderer, SceneUpdates,
    rgb565::{Rgb565Be, Rgb565Le},
    scene_store::SceneStore,
    text_fonts::TextFonts,
};
use cranpose_ui_graphics::{MemoryHeadroom, Size};
pub use draw::draw_scene;
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

/// The picture a host drew last. [`PixelsRenderer::update_picture`] moves it
/// to the current scene and finds the pixels that change, and
/// [`PixelsRenderer::draw_picture_area`] draws any part of it.
#[derive(Default)]
pub struct Picture {
    raster: Option<scene::RasterScene>,
    /// What an update in place pushes each changed layer again with.
    rewrite: Option<Box<rewrite::Rewrite>>,
    width: u32,
    height: u32,
    scale: f32,
    changed: Vec<draw::ClipBounds>,
    // Where each item of `raster` can draw in the frame, in its order: each
    // strip a host draws passes over the items that miss it.
    reach: Vec<Option<draw::ClipBounds>>,
    // The store and its update the scene was brought up to.
    synced: Option<(StoreId, SceneUpdate)>,
    // The subtrees an update in place pushes again, kept for its room.
    roots: Vec<pipeline::PushRoot>,
}

// The most records and updates the changes keep, the latest ones: a picture
// that lags past them builds its scene again. Once they hold `MOST_UPDATES`
// they forget all but the latest `KEPT_UPDATES`, so an update seldom pays
// for the forgetting.
const MOST_CHANGES: usize = 256;
const MOST_UPDATES: usize = 32;
const KEPT_UPDATES: usize = 8;

// The records the store changed in its latest updates, oldest first: a
// picture that holds the scene of one of those updates pushes again the
// records changed since.
#[derive(Default)]
struct StoreChanges {
    // The store's update when its changes were read last.
    at: Option<(StoreId, SceneUpdate)>,
    // Each update the changes reach back to, with where the records changed
    // after it start.
    updates: Vec<(SceneUpdate, usize)>,
    records: Vec<RecordRef>,
}

impl StoreChanges {
    // Adds what the latest update of `store` changed.
    fn read(&mut self, store: &SceneStore) {
        let now = (store.id(), store.update());
        if self.at == Some(now) {
            return;
        }
        let touched = store.touched();
        if !touched.is_some_and(|(since, _)| self.at == Some((now.0, since))) {
            self.updates.clear();
            self.records.clear();
        }
        if let Some((since, records)) = touched {
            if self.updates.len() == MOST_UPDATES
                || self.records.len() + records.len() > MOST_CHANGES
            {
                self.forget_oldest(records.len());
            }
            self.updates.push((since, self.records.len()));
            self.records.extend_from_slice(records);
        }
        self.at = Some(now);
    }

    // Forgets the oldest updates: all but the latest `KEPT_UPDATES`, and
    // more of them until half of the records' room is free after `more`
    // records, so updates that change many records seldom come here too.
    #[cold]
    fn forget_oldest(&mut self, more: usize) {
        let records = self.records.len();
        let start = |gone: usize| self.updates.get(gone).map_or(records, |update| update.1);
        let mut gone = self.updates.len().saturating_sub(KEPT_UPDATES);
        while gone < self.updates.len() && records - start(gone) + more > MOST_CHANGES / 2 {
            gone += 1;
        }
        let cut = start(gone);
        self.records.drain(..cut);
        self.updates.drain(..gone);
        for update in &mut self.updates {
            update.1 -= cut;
        }
    }

    // The records `store` changed since `synced`, when the changes reach
    // back to it and up to the store's update.
    fn since<'a>(
        &'a self,
        store: &'a SceneStore,
        synced: Option<(StoreId, SceneUpdate)>,
    ) -> Option<pipeline::Changed<'a>> {
        let now = (store.id(), store.update());
        if synced == Some(now) {
            return Some(pipeline::Changed {
                store,
                records: &[],
                updates: 0,
            });
        }
        let (id, update) = synced?;
        if self.at != Some(now) || id != now.0 {
            return None;
        }
        // A picture mostly lags by one update, the last of the list.
        let &(_, start) = self.updates.iter().rfind(|span| span.0 == update)?;
        Some(pipeline::Changed {
            store,
            records: &self.records[start..],
            updates: now.1.since(update),
        })
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
    changes: StoreChanges,
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
            changes: StoreChanges::default(),
            text: draw::TextRaster::new(TextFonts::from_fonts_or_default(fonts)),
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
        if self.scene.root_layer().is_some() {
            pipeline::fill_raster_scene(
                raster,
                self.scene.store(),
                self.scene.diagnostics(),
                scale,
                previous,
                &self.text,
            );
        } else {
            raster.clear();
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
        let store = self.scene.store();
        let now = Some((store.id(), store.update()));
        if let Some(reach) = self.update_picture_in_place(picture, width, height, scale) {
            picture.synced = now;
            changed.clear();
            changed.extend(picture.changed.iter().copied().map(pixel_rect));
            match reach {
                pipeline::UpdatedReach::Held => {}
                // A push that lost records lost the reach too: only then can
                // the scene keep the room of many more records than it holds.
                pipeline::UpdatedReach::Lost => {
                    picture.measure_reach();
                    if growth_trim::shrank(picture) {
                        growth_trim::trim(picture);
                    }
                }
            }
            // With little memory left, the spill an update took for a
            // subtree that pushed other items than before goes, and so does
            // the room the scene's lists grew past their items.
            if cranpose_ui::current_memory_headroom() == MemoryHeadroom::Tight {
                if let Some(rewrite) = picture.rewrite.as_mut() {
                    rewrite.release_spill();
                }
                growth_trim::trim(picture);
            }
            return;
        }
        self.redraw_picture(picture, width, height, scale, changed);
        picture.synced = now;
    }

    // Builds `picture`'s scene again and puts into `changed` the areas that
    // differ from the scene before. Out of line: most frames update the
    // scene in place.
    #[inline(never)]
    fn redraw_picture(
        &self,
        picture: &mut Picture,
        width: u32,
        height: u32,
        scale: f32,
        changed: &mut Vec<PixelRect>,
    ) {
        // An update in place that gave up after it changed part of the scene
        // left the areas it changed: the scene holds those changes already.
        let updated = (!picture.changed.is_empty()).then(|| std::mem::take(&mut picture.changed));
        let mut raster = picture
            .rewrite
            .as_mut()
            .map(|rewrite| rewrite.take_storage())
            .unwrap_or_default();
        self.fill_raster_scene(&mut raster, scale, picture.raster.as_ref());
        changed.clear();
        match picture.raster.as_ref() {
            Some(old)
                if (picture.width, picture.height, picture.scale) == (width, height, scale) =>
            {
                damage::changed_areas(
                    old,
                    &picture.reach,
                    &raster,
                    picture.frame(),
                    &mut picture.changed,
                );
                if let Some(mut updated) = updated {
                    picture.changed.append(&mut updated);
                    damage::merge_areas(&mut picture.changed);
                }
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
        picture.raster = Some(raster);
        picture.width = width;
        picture.height = height;
        picture.scale = scale;
        picture.measure_reach();
        if cranpose_ui::current_memory_headroom() == MemoryHeadroom::Tight
            || growth_trim::shrank(picture)
        {
            growth_trim::trim(picture);
        }
    }

    // Brings `picture`'s scene up to the current scene in place, and puts
    // the areas the changed layers changed into `picture.changed`. `None`
    // when the scene must be built again: the first time, after a change of
    // size, scale or light, or of the root layer, when the picture holds
    // another scene than the one before the store's last update, or when a
    // layer's records no longer lie in one run, with the areas the update
    // changed before it stopped in `picture.changed`.
    fn update_picture_in_place(
        &self,
        picture: &mut Picture,
        width: u32,
        height: u32,
        scale: f32,
    ) -> Option<pipeline::UpdatedReach> {
        picture.changed.clear();
        let (Some(raster), Some(_)) = (picture.raster.as_mut(), self.scene.root_layer()) else {
            return None;
        };
        if (picture.width, picture.height, picture.scale) != (width, height, scale) {
            return None;
        }
        let changed = self.changes.since(self.scene.store(), picture.synced)?;
        let window = draw::ClipBounds {
            min_x: 0,
            min_y: 0,
            max_x: i32::try_from(width).unwrap_or(i32::MAX),
            max_y: i32::try_from(height).unwrap_or(i32::MAX),
        };
        pipeline::InPlaceUpdate {
            scene: raster,
            reach: &mut picture.reach,
            rewrite: &mut picture.rewrite,
            roots: &mut picture.roots,
        }
        .run(
            changed,
            self.scene.diagnostics(),
            scale,
            &self.text,
            window,
            &mut picture.changed,
        )
    }

    /// Rasterizes the glyphs of the text in `area` of `picture` into the
    /// renderer's glyph cache, so that drawing the area later takes them from
    /// it. A host calls it while it waits anyway, such as for a display that
    /// is still starting up.
    pub fn prepare_picture_area(&self, picture: &Picture, area: PixelRect) {
        let Some(raster) = picture.raster.as_ref() else {
            return;
        };
        let window = draw::ClipBounds {
            min_x: i32::try_from(area.x).unwrap_or(i32::MAX),
            min_y: i32::try_from(area.y).unwrap_or(i32::MAX),
            max_x: i32::try_from(area.x.saturating_add(area.width)).unwrap_or(i32::MAX),
            max_y: i32::try_from(area.y.saturating_add(area.height)).unwrap_or(i32::MAX),
        };
        draw::prepare_raster_window(raster, window, &self.text);
    }

    /// Draws `area` of `picture` into `pixels` as straight-alpha RGBA8 rows of
    /// `area.width` pixels, `area.height` of them. Draws nothing before the
    /// first [`Self::update_picture`] or when `pixels` is shorter than the area.
    pub fn draw_picture_area(&self, picture: &Picture, area: PixelRect, pixels: &mut [u8]) {
        self.draw_picture_area_as(picture, area, pixels.as_chunks_mut::<4>().0);
    }

    /// Draws `area` of `picture` into `pixels` as rows of 16-bit pixels, 5
    /// bits of red, 6 of green and 5 of blue, stored in `order`, as a display
    /// takes them: `area.width` pixels a row, `area.height` rows. Each pixel
    /// keeps the high bits of the bytes [`Self::draw_picture_area`] draws,
    /// except where a drawing blends over another, which blends over the
    /// 16-bit pixel.
    pub fn draw_picture_area_rgb565(
        &self,
        picture: &Picture,
        area: PixelRect,
        pixels: &mut [u16],
        order: Rgb565Order,
    ) {
        match order {
            Rgb565Order::LittleEndian => self.draw_picture_area_as::<Rgb565Le>(
                picture,
                area,
                bytemuck::cast_slice_mut(pixels),
            ),
            Rgb565Order::BigEndian => self.draw_picture_area_as::<Rgb565Be>(
                picture,
                area,
                bytemuck::cast_slice_mut(pixels),
            ),
        }
    }

    // Out of line, one function a format: a display takes one byte order, and
    // a build that keeps its drawing in fast memory keeps only that one.
    #[inline(never)]
    fn draw_picture_area_as<P: Pixel>(&self, picture: &Picture, area: PixelRect, pixels: &mut [P]) {
        let Some(raster) = picture.raster.as_ref() else {
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
            &picture.reach,
            self.scene.diagnostics(),
            &self.text,
        );
    }

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

impl Picture {
    fn frame(&self) -> draw::ClipBounds {
        draw::ClipBounds {
            min_x: 0,
            min_y: 0,
            max_x: i32::try_from(self.width).unwrap_or(i32::MAX),
            max_y: i32::try_from(self.height).unwrap_or(i32::MAX),
        }
    }

    fn measure_reach(&mut self) {
        let frame = self.frame();
        match self.raster.as_ref() {
            Some(raster) => raster.reach_into(frame, &mut self.reach),
            None => self.reach.clear(),
        }
    }
}

impl Renderer for PixelsRenderer {
    type Scene = Scene;
    type Error = std::convert::Infallible;

    fn attach_app_context_services(&mut self, app_context: &cranpose_ui::AppContext) {
        app_context.set_text_fonts(self.text.fonts.resolver());
        self.app_context = Some(app_context.downgrade());
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
            self.attach_app_context_services(app_context);
        }
    }

    fn scene(&self) -> &Self::Scene {
        &self.scene
    }

    fn scene_mut(&mut self) -> &mut Self::Scene {
        &mut self.scene
    }

    fn rebuild_scene_from_applier(
        &mut self,
        applier: &mut cranpose_core::MemoryApplier,
        root: cranpose_core::NodeId,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.scene.rebuild_from_applier(applier, root);
        self.changes.read(self.scene.store());
        Ok(())
    }

    fn update_scene_from_applier(
        &mut self,
        applier: &mut cranpose_core::MemoryApplier,
        root: cranpose_core::NodeId,
        _viewport: Size,
        updates: SceneUpdates<'_>,
    ) -> Result<(), Self::Error> {
        self.scene.update_from_applier(applier, root, updates);
        self.changes.read(self.scene.store());
        Ok(())
    }
}
