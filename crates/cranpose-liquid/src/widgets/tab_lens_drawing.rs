use std::{cell::RefCell, rc::Rc};

use cranpose_ui::Modifier;

use super::{
    tab_bar::{TabBarMotion, TabLensPlacement, tab_bar_transform, tab_selection_content_color},
    tab_cells::tab_ink_selection,
    tab_flight_lens::{request_touch_warm_ups, tab_flight_dynamics, tab_flight_lens_material},
};
use crate::{material::GlassDynamics, theme::LiquidColors};

/// The flying lens's two glass layers for one material, kept while the lens's
/// activity, which picks the material, holds.
type TabLensLayers = dyn Fn(f32, GlassDynamics) -> [cranpose_ui_graphics::GraphicsLayer; 2];

/// The lens layers held for the activity, by its bits, that picked them.
type HeldTabLensLayers = (u32, Rc<TabLensLayers>);

/// Draws the flying lens and the ink under it from the bar's moving parts,
/// each frame, without recomposing the bar.
pub(super) struct TabLensDrawing {
    lens: TabLensPlacement,
    motion: TabBarMotion,
    colors: LiquidColors,
    held_layers: Rc<RefCell<Option<HeldTabLensLayers>>>,
}

impl TabLensDrawing {
    pub(super) fn remember(
        lens: TabLensPlacement,
        motion: TabBarMotion,
        colors: LiquidColors,
    ) -> Rc<Self> {
        request_touch_warm_ups(&colors, lens.accessory_center);
        Rc::new(Self {
            lens,
            motion,
            colors,
            held_layers: cranpose_core::remember(|| Rc::new(RefCell::new(None))).with(Rc::clone),
        })
    }

    /// The bar's lift this frame.
    pub(super) fn transform(&self) -> cranpose_ui_graphics::GraphicsLayer {
        tab_bar_transform(
            self.lens.pill_w,
            self.motion.bar_press.get(),
            self.motion.travel.get(),
        )
    }

    /// Where the lens's ink falls on the cells this frame.
    pub(super) fn selection(&self) -> super::vibrancy::InkSelection {
        let frame = self
            .motion
            .frame(self.lens.cells, || self.lens.resting_lens_x());
        tab_ink_selection(
            self.lens.cells,
            frame.lens_x,
            frame.activity,
            frame.strain,
            tab_selection_content_color(self.colors),
        )
    }

    /// The glass layers for the lens's activity, built again only when the
    /// activity picks another material.
    fn layers(&self, activity: f32) -> Rc<TabLensLayers> {
        let key = activity.to_bits();
        let mut held = self.held_layers.borrow_mut();
        if let Some((held_key, layers)) = &*held
            && *held_key == key
        {
            return Rc::clone(layers);
        }
        let layers: Rc<TabLensLayers> = Rc::new(crate::material::cached_glass_content_layers(
            tab_flight_lens_material(self.colors.label, activity).resolve(&self.colors),
        ));
        *held = Some((key, Rc::clone(&layers)));
        layers
    }

    /// One of the lens's two glass layers this frame: 0 under the cells, 1 over them.
    fn glass_layer(&self, layer: usize) -> cranpose_ui_graphics::GraphicsLayer {
        let frame = self
            .motion
            .frame(self.lens.cells, || self.lens.resting_lens_x());
        std::mem::take(
            &mut self.layers(frame.activity)(
                cranpose_ui::current_density(),
                tab_flight_dynamics(self.lens.geometry(frame), self.lens.node(frame)),
            )[layer],
        )
    }

    /// The lens node carried across the bar, drawing one of its glass layers.
    /// It is lowered to its top by translation, not by a layout offset,
    /// which would land it on a whole device pixel: the native lens sits
    /// between pixels.
    pub(super) fn glass_modifier(self: &Rc<Self>, layer: usize) -> Modifier {
        let (node, glass) = (Rc::clone(self), Rc::clone(self));
        let node_top = self.lens.node_top;
        Modifier::empty()
            .required_size(self.lens.node_size)
            .graphics_layer(move || {
                let mut layer = node.lens.layer(&node.motion);
                layer.translation_y += node_top;
                layer
            })
            .graphics_layer(move || glass.glass_layer(layer))
    }
}
