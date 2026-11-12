// The properties a record keeps beyond its fixed-size parts: a graphics
// layer, pointer handling, layer bounds and what else `LayerProps` holds.

use cranpose_core::RecordId;
use cranpose_ui::{GraphicsLayer, Rect};

use super::{GraphicsLayerId, LayerProps, PropsId, RecordFlags, SceneStore};
use crate::graph::HitTestNode;

#[expect(clippy::multiple_inherent_impl, reason = "one file per concern")]
impl SceneStore {
    /// The properties of `id`, when it has any.
    #[inline]
    pub fn props(&self, id: RecordId) -> Option<&LayerProps> {
        let index = id.index();
        if !self.tree[index].flags.contains(RecordFlags::HAS_PROPS) {
            return None;
        }
        self.warm[index]
            .props
            .map(|props| &self.props[props.index()])
    }

    // Removes the properties and the graphics layer of `id` for the scene
    // builder, which notes the change itself.
    pub(crate) fn strip_props(&mut self, id: RecordId) {
        let index = id.index();
        if self.warm[index].props.is_some() || self.warm[index].graphics_layer.is_some() {
            self.drop_props(index);
            super::facts::forget_children(self, index);
            self.stale_with_parent(index);
        }
    }

    // Removes the properties and the graphics layer of the record at
    // `index`.
    pub(super) fn drop_props(&mut self, index: usize) {
        if let Some(entry) = self.warm[index].props.take() {
            self.props.remove(entry.index());
        }
        if let Some(slot) = self.warm[index].graphics_layer.take() {
            self.graphics_layers.remove(slot.index());
        }
        self.tree[index].flags = self.tree[index].flags.without(
            RecordFlags::HAS_PROPS
                | RecordFlags::HAS_GRAPHICS_LAYER
                | RecordFlags::HAS_LAYER_BOUNDS,
        );
    }

    // Sets the flags that tell whether the record at `index` has properties
    // and layer bounds, from them.
    fn sync_props_flags(&mut self, index: usize) {
        let props = self.warm[index]
            .props
            .map(|entry| &self.props[entry.index()]);
        let bounds = props.is_some_and(|props| props.layer_bounds.is_some());
        let flags = self.tree[index]
            .flags
            .without(RecordFlags::HAS_PROPS | RecordFlags::HAS_LAYER_BOUNDS);
        let flags = if props.is_some() {
            flags.with(RecordFlags::HAS_PROPS)
        } else {
            flags
        };
        self.tree[index].flags = if bounds {
            flags.with(RecordFlags::HAS_LAYER_BOUNDS)
        } else {
            flags
        };
    }

    /// The graphics layer set on `id`, `None` when it reads as the default.
    #[inline]
    pub fn graphics_layer(&self, id: RecordId) -> Option<&GraphicsLayer> {
        let index = id.index();
        if !self.tree[index]
            .flags
            .contains(RecordFlags::HAS_GRAPHICS_LAYER)
        {
            return None;
        }
        self.warm[index]
            .graphics_layer
            .map(|slot| &self.graphics_layers[slot.index()])
    }

    /// Sets the graphics layer of `id`; `None` reads as the default. A record
    /// that keeps one keeps its slot. A changed layer notes a property
    /// change.
    pub fn set_graphics_layer(&mut self, id: RecordId, layer: Option<GraphicsLayer>) {
        if self.graphics_layer(id) != layer.as_ref() {
            self.write_graphics_layer(id, layer);
            self.note_property_change(id);
        }
    }

    // Writes the graphics layer of `id` for the scene builder, which notes
    // the change itself.
    #[inline]
    pub(crate) fn write_graphics_layer(&mut self, id: RecordId, layer: Option<GraphicsLayer>) {
        // Most records keep no graphics layer.
        if layer.is_some() || self.warm[id.index()].graphics_layer.is_some() {
            self.replace_graphics_layer(id, layer);
        }
    }

    #[inline(never)]
    fn replace_graphics_layer(&mut self, id: RecordId, layer: Option<GraphicsLayer>) {
        let index = id.index();
        let slot = self.warm[index].graphics_layer;
        let space =
            |layer: Option<&GraphicsLayer>| layer.is_some_and(|layer| layer.space.is_some());
        if space(slot.map(|slot| &self.graphics_layers[slot.index()])) != space(layer.as_ref()) {
            super::facts::forget_children(self, id.index());
        }
        match (slot, layer) {
            (Some(slot), Some(layer)) => {
                let current = &self.graphics_layers[slot.index()];
                if super::facts::layer_inputs(current) != super::facts::layer_inputs(&layer) {
                    self.stale_with_parent(id.index());
                }
                self.graphics_layers[slot.index()] = layer;
            }
            (Some(slot), None) => {
                self.graphics_layers.remove(slot.index());
                self.warm[index].graphics_layer = None;
                let flags = &mut self.tree[index].flags;
                *flags = flags.without(RecordFlags::HAS_GRAPHICS_LAYER);
                self.stale_with_parent(index);
            }
            (None, Some(layer)) => {
                let slot = GraphicsLayerId::from_index(self.graphics_layers.insert(layer));
                self.warm[index].graphics_layer = Some(slot);
                let flags = &mut self.tree[index].flags;
                *flags = flags.with(RecordFlags::HAS_GRAPHICS_LAYER);
                self.stale_with_parent(index);
            }
            (None, None) => {}
        }
    }

    // Writes the layer bounds of `id` for the scene builder, which notes the
    // change itself.
    pub(crate) fn write_layer_bounds(&mut self, id: RecordId, bounds: Option<Rect>) {
        let bits = |bounds: Option<Rect>| {
            bounds.map(|bounds| [bounds.x, bounds.y, bounds.width, bounds.height].map(f32::to_bits))
        };
        if bits(self.props(id).and_then(|props| props.layer_bounds)) != bits(bounds) {
            self.edit_props(id, |props| props.layer_bounds = bounds);
        }
    }

    /// Sets the pointer handling of `id`: `None` when it has none. What the
    /// record draws stays.
    pub fn set_hit_test(&mut self, id: RecordId, hit_test: Option<HitTestNode>) {
        if hit_test.is_some() || self.props(id).is_some_and(|props| props.hit_test.is_some()) {
            self.edit_props(id, |props| props.hit_test = hit_test);
        }
    }

    // Changes the properties of `id` with `edit`, starting from defaults
    // when it has none, for the scene builder, which notes the change
    // itself. Properties left at their defaults go.
    pub(crate) fn edit_props(&mut self, id: RecordId, edit: impl FnOnce(&mut LayerProps)) {
        let index = id.index();
        let entry = self.warm[index].props;
        let mut fresh = None;
        let props = match entry {
            Some(entry) => &mut self.props[entry.index()],
            None => fresh.insert(LayerProps::default()),
        };
        let inputs =
            |props: &LayerProps| (super::facts::props_inputs(props), props.hit_test.is_some());
        let before = inputs(props);
        edit(props);
        let after = inputs(props);
        match (entry, props.is_default(), fresh) {
            (Some(entry), true, _) => {
                self.props.remove(entry.index());
                self.warm[index].props = None;
            }
            (None, false, Some(props)) => {
                self.warm[index].props = Some(PropsId::from_index(self.props.insert(props)));
            }
            _ => {}
        }
        if self.warm[index].props.is_some() != entry.is_some() {
            self.sync_props_flags(index);
        }
        // The other flags and the facts follow from the inputs alone.
        if before == after {
            return;
        }
        self.sync_props_flags(index);
        if before.1 != after.1 {
            super::facts::forget_children(self, index);
        }
        if before.0 != after.0 {
            self.stale_with_parent(index);
        }
    }
}
