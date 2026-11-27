// The list as `ScalingLazyColumnStateAdapter` reads it, in device pixels.
//
// Wear measures its thumb in fractional item indices rather than in pixels.
// The measure pass fills this buffer so the indicator can read the window and
// auto-centring spacers without extra allocations during draw.

use super::{ItemHeights, WearScalingLazyColumnSpec, WindowedRow};
use crate::{
    density::Density,
    round_scaling_list::{leading_auto_centring_spacer, trailing_auto_centring_spacer},
    round_scroll_indicator::{IndicatorItem, ThumbLength, scaling_list_items_with},
};

#[derive(Debug, Default)]
pub(super) struct IndicatorState {
    pub(super) visible: Vec<IndicatorItem>,
    pub(super) total: usize,
    pub(super) viewport: f32,
    pub(super) before_padding: f32,
    pub(super) after_padding: f32,
    pub(super) thumb: ThumbLength,
}

impl IndicatorState {
    pub(super) fn clear(&mut self) {
        self.visible.clear();
        self.total = 0;
        self.viewport = 0.0;
        self.before_padding = 0.0;
        self.after_padding = 0.0;
    }

    pub(super) fn record(
        &mut self,
        window: &[WindowedRow],
        spec: &WearScalingLazyColumnSpec,
        known: &ItemHeights,
        viewport: f32,
        density: Density,
    ) {
        let count = known.len();
        if count == 0 {
            self.clear();
            return;
        }
        let viewport_px = density.to_px(viewport).round();
        self.total = count;
        self.viewport = viewport_px;
        scaling_list_items_with(
            spec.scaling,
            viewport,
            density.density(),
            window.iter().map(|row| (row.top, row.height)),
            &mut self.visible,
        );
        if let Some(base) = window.first().map(|row| row.index) {
            for item in &mut self.visible {
                item.index += base;
            }
        }
        let mut before = density.to_px(density.dp(spec.content_padding.top));
        let mut after = density.to_px(density.dp(spec.content_padding.bottom));
        if let Some(anchor) = spec.auto_centering {
            let index = anchor.index.min(count - 1);
            let spacing = density.dp(spec.item_spacing);
            let centre = (0..index)
                .map(|i| known.height_of(i) + spacing)
                .sum::<f32>()
                + known.height_of(index) * 0.5;
            before += leading_auto_centring_spacer(
                viewport_px,
                density.to_px(centre).round(),
                density.to_px(anchor.offset),
            );
            after += trailing_auto_centring_spacer(
                viewport_px,
                density.to_px(known.height_of(count - 1)).round(),
            );
        }
        self.before_padding = before;
        self.after_padding = after;
    }
}
