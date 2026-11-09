// The properties a record keeps beyond its fixed-size parts: a graphics
// layer, pointer handling, layer bounds and what else `LayerProps` holds.

use cranpose_core::RecordId;
use cranpose_ui::GraphicsLayer;

use super::{GraphicsLayerId, LayerProps, PropsId, RecordFlags, SceneStore};

#[expect(clippy::multiple_inherent_impl, reason = "one file per concern")]
impl SceneStore {
    /// The properties of `id`, when it has any.
    pub fn props(&self, id: RecordId) -> Option<&LayerProps> {
        self.warm[id.index()]
            .props
            .map(|props| &self.props[props.index()])
    }

    /// Removes the properties of `id`.
    pub fn clear_props(&mut self, id: RecordId) {
        let index = id.index();
        if self.warm[index].props.is_some() {
            self.drop_props(index);
            super::facts::forget_children(self, index);
            self.stale_with_parent(index);
        }
    }

    pub(super) fn drop_props(&mut self, index: usize) {
        let Some(entry) = self.warm[index].props.take() else {
            return;
        };
        if let Some(slot) = self
            .props
            .remove(entry.index())
            .and_then(|props| props.graphics_layer)
        {
            self.graphics_layers.remove(slot.index());
        }
        self.tree[index].flags = self.tree[index]
            .flags
            .without(RecordFlags::HAS_GRAPHICS_LAYER | RecordFlags::HAS_LAYER_BOUNDS);
    }

    // Sets the flags that tell what the properties of the record at `index`
    // hold, from them.
    fn sync_props_flags(&mut self, index: usize) {
        let (layer, bounds) = self.warm[index]
            .props
            .map(|entry| &self.props[entry.index()])
            .map_or((false, false), |props| {
                (props.graphics_layer.is_some(), props.layer_bounds.is_some())
            });
        let flags = self.tree[index]
            .flags
            .without(RecordFlags::HAS_GRAPHICS_LAYER | RecordFlags::HAS_LAYER_BOUNDS);
        let flags = if layer {
            flags.with(RecordFlags::HAS_GRAPHICS_LAYER)
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
    pub fn graphics_layer(&self, id: RecordId) -> Option<&GraphicsLayer> {
        if !self.tree[id.index()]
            .flags
            .contains(RecordFlags::HAS_GRAPHICS_LAYER)
        {
            return None;
        }
        self.props(id)
            .and_then(|props| props.graphics_layer)
            .map(|slot| &self.graphics_layers[slot.index()])
    }

    /// Sets the graphics layer of `id`; `None` reads as the default. A record
    /// that keeps one keeps its slot.
    pub fn set_graphics_layer(&mut self, id: RecordId, layer: Option<GraphicsLayer>) {
        let slot = self.props(id).and_then(|props| props.graphics_layer);
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
                self.edit_props(id, |props| props.graphics_layer = None);
                self.stale_with_parent(id.index());
            }
            (None, Some(layer)) => {
                let slot = GraphicsLayerId::from_index(self.graphics_layers.insert(layer));
                self.edit_props(id, |props| props.graphics_layer = Some(slot));
                self.stale_with_parent(id.index());
            }
            (None, None) => {}
        }
    }

    /// Changes the properties of `id` with `edit`, starting from defaults
    /// when it has none; properties left at their defaults go.
    pub fn edit_props(&mut self, id: RecordId, edit: impl FnOnce(&mut LayerProps)) {
        let index = id.index();
        let entry = self.warm[index].props;
        let mut fresh = None;
        let props = match entry {
            Some(entry) => &mut self.props[entry.index()],
            None => fresh.insert(LayerProps::default()),
        };
        let before = (super::facts::props_inputs(props), props.hit_test.is_some());
        edit(props);
        let after = (super::facts::props_inputs(props), props.hit_test.is_some());
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
        self.sync_props_flags(index);
        if before.1 != after.1 {
            super::facts::forget_children(self, index);
        }
        if before.0 != after.0 {
            self.stale_with_parent(index);
        }
    }
}
