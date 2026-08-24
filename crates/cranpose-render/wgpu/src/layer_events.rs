use std::ops::Range;

use crate::{
    normalized_scene::effect_layer_in_range,
    scene::{BackdropLayer, EffectLayer},
};

#[derive(Clone, Copy)]
pub(crate) enum LayerEventKind {
    Backdrop(usize),
    Effect(usize),
}

#[derive(Clone, Copy)]
pub(crate) struct LayerEvent {
    pub(crate) z_index: usize,
    pub(crate) kind: LayerEventKind,
}

impl LayerEvent {
    fn kind_order(self) -> u8 {
        match self.kind {
            // Backdrop must run before same-z content/effects so it samples only
            // already-rendered background.
            LayerEventKind::Backdrop(_) => 0,
            LayerEventKind::Effect(_) => 1,
        }
    }
}

pub(crate) fn collect_effect_ranges(
    effect_layers: &[EffectLayer],
    z_start: usize,
    z_end: usize,
    excluded_effect_layer: Option<usize>,
    out: &mut Vec<Range<usize>>,
) {
    out.clear();
    for (index, layer) in effect_layers.iter().enumerate() {
        if Some(index) == excluded_effect_layer {
            continue;
        }
        if effect_layer_in_range(layer, z_start, z_end) {
            out.push(layer.z_start..layer.z_end);
        }
    }
    out.sort_by_key(|range| range.start);
}

pub(crate) fn collect_layer_events(
    effect_layers: &[EffectLayer],
    backdrop_layers: &[BackdropLayer],
    z_start: usize,
    z_end: usize,
    excluded_effect_layer: Option<usize>,
    out: &mut Vec<LayerEvent>,
) {
    out.clear();
    for (index, layer) in backdrop_layers.iter().enumerate() {
        if layer.z_index >= z_start && layer.z_index < z_end {
            out.push(LayerEvent {
                z_index: layer.z_index,
                kind: LayerEventKind::Backdrop(index),
            });
        }
    }
    for (index, layer) in effect_layers.iter().enumerate() {
        if Some(index) == excluded_effect_layer {
            continue;
        }
        if effect_layer_in_range(layer, z_start, z_end) {
            out.push(LayerEvent {
                z_index: layer.z_start,
                kind: LayerEventKind::Effect(index),
            });
        }
    }
    out.sort_by(|a, b| {
        let z_cmp = a.z_index.cmp(&b.z_index);
        if z_cmp != std::cmp::Ordering::Equal {
            return z_cmp;
        }

        let kind_cmp = a.kind_order().cmp(&b.kind_order());
        if kind_cmp != std::cmp::Ordering::Equal {
            return kind_cmp;
        }

        match (a.kind, b.kind) {
            (LayerEventKind::Effect(ai), LayerEventKind::Effect(bi)) => effect_layers[bi]
                .z_end
                .cmp(&effect_layers[ai].z_end)
                .then_with(|| bi.cmp(&ai)),
            _ => std::cmp::Ordering::Equal,
        }
    });
}
