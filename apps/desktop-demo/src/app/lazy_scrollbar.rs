use cranpose_foundation::SemanticsConfiguration;
use cranpose_ui::{
    composable, Box as UiBox, BoxSpec, Brush, Color, LinearArrangement, Modifier, Row, RowSpec,
};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LazyScrollbarStyle {
    pub rail_width: f32,
    pub thumb_width: f32,
    pub min_thumb_height: f32,
    pub rail_color: Color,
    pub thumb_color: Color,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LazyScrollbarModel {
    pub total_items: usize,
    pub average_item_size: f32,
    pub max_item_position: f32,
    pub thumb_fraction: f32,
    pub scroll_fraction: f32,
}

impl Default for LazyScrollbarModel {
    fn default() -> Self {
        Self {
            total_items: 0,
            average_item_size: 1.0,
            max_item_position: 0.0,
            thumb_fraction: 1.0,
            scroll_fraction: 0.0,
        }
    }
}

pub(crate) fn compute_scrollbar_metrics(
    rail_height: f32,
    thumb_fraction: f32,
    scroll_fraction: f32,
    min_thumb_height: f32,
) -> (f32, f32) {
    let h = rail_height.max(1.0);
    let thumb_h = (thumb_fraction * h).max(min_thumb_height).min(h);
    let thumb_range = (h - thumb_h).max(0.0);
    let thumb_y = scroll_fraction.clamp(0.0, 1.0) * thumb_range;
    (thumb_h, thumb_y)
}

pub(crate) fn compute_scrollbar_model(
    total_items: usize,
    viewport_size: f32,
    average_item_size: f32,
    first_visible_index: usize,
    first_visible_offset: f32,
) -> LazyScrollbarModel {
    let average_item_size = average_item_size.max(1.0);
    let viewport_size = viewport_size.max(1.0);

    if total_items == 0 {
        return LazyScrollbarModel {
            total_items,
            average_item_size,
            max_item_position: 0.0,
            thumb_fraction: 1.0,
            scroll_fraction: 0.0,
        };
    }

    let estimated_visible_items = (viewport_size / average_item_size).max(1.0);
    let max_item_position = (total_items as f32 - estimated_visible_items).max(0.0);
    let current_item_position = if max_item_position > 0.0 {
        (first_visible_index as f32 + (first_visible_offset / average_item_size))
            .clamp(0.0, max_item_position)
    } else {
        0.0
    };

    let thumb_fraction = (estimated_visible_items / total_items as f32).clamp(0.04, 1.0);
    let scroll_fraction = if max_item_position > 0.0 {
        current_item_position / max_item_position
    } else {
        0.0
    };

    LazyScrollbarModel {
        total_items,
        average_item_size,
        max_item_position,
        thumb_fraction,
        scroll_fraction,
    }
}

pub(crate) fn scroll_target_for_fraction(
    model: LazyScrollbarModel,
    scroll_fraction: f32,
) -> (usize, f32) {
    if model.total_items == 0 || model.max_item_position <= 0.0 {
        return (0, 0.0);
    }

    let target_position = scroll_fraction.clamp(0.0, 1.0) * model.max_item_position;
    let mut index = target_position.floor() as usize;
    index = index.min(model.total_items.saturating_sub(1));
    let offset = ((target_position - index as f32) * model.average_item_size).max(0.0);
    (index, offset)
}

pub(crate) fn average_visible_item_size(
    layout_info: &cranpose_foundation::lazy::LazyListLayoutInfo,
    fallback_average_item_size: f32,
) -> f32 {
    if layout_info.visible_items_info.is_empty() {
        return fallback_average_item_size.max(1.0);
    }

    let total_size: f32 = layout_info
        .visible_items_info
        .iter()
        .map(|item| item.size.max(1.0))
        .sum();
    (total_size / layout_info.visible_items_info.len() as f32).max(1.0)
}

pub(crate) fn stabilize_scrollbar_model_for_scrollable_content(
    mut model: LazyScrollbarModel,
    can_scroll_forward: bool,
    can_scroll_backward: bool,
) -> LazyScrollbarModel {
    if model.total_items == 0 {
        return model;
    }
    if model.max_item_position > 0.0 {
        return model;
    }
    if !can_scroll_forward && !can_scroll_backward {
        return model;
    }

    model.max_item_position = 1.0;
    model.thumb_fraction = model.thumb_fraction.min(0.98);
    model.scroll_fraction = match (can_scroll_backward, can_scroll_forward) {
        (false, true) => 0.0,
        (true, false) => 1.0,
        (true, true) => 0.5,
        (false, false) => 0.0,
    };
    model
}

fn read_interaction_scrollbar_model(
    list_state: cranpose_foundation::lazy::LazyListState,
) -> (LazyScrollbarModel, f32) {
    let info = list_state.layout_info();
    let model = compute_scrollbar_model(
        info.total_items_count,
        info.viewport_size,
        average_visible_item_size(&info, list_state.average_item_size()),
        list_state.first_visible_item_index(),
        list_state.first_visible_item_scroll_offset(),
    );
    let model = stabilize_scrollbar_model_for_scrollable_content(
        model,
        list_state.can_scroll_forward(),
        list_state.can_scroll_backward(),
    );
    let rail_height = info.viewport_size.max(1.0);
    (model, rail_height)
}

#[allow(non_snake_case)]
#[composable]
fn LazyScrollbarModelObserver(
    list_state: cranpose_foundation::lazy::LazyListState,
    model_state: cranpose_core::MutableState<LazyScrollbarModel>,
) {
    let first_visible_index = list_state.first_visible_item_index();
    let first_visible_offset = list_state.first_visible_item_scroll_offset();
    let layout_info = list_state.layout_info();
    let can_scroll_forward = list_state.can_scroll_forward();
    let can_scroll_backward = list_state.can_scroll_backward();
    let avg_item_size = average_visible_item_size(&layout_info, list_state.average_item_size());
    let next_model = compute_scrollbar_model(
        layout_info.total_items_count,
        layout_info.viewport_size,
        avg_item_size,
        first_visible_index,
        first_visible_offset,
    );
    let next_model = stabilize_scrollbar_model_for_scrollable_content(
        next_model,
        can_scroll_forward,
        can_scroll_backward,
    );

    if model_state.get_non_reactive() != next_model {
        cranpose_core::SideEffect(move || {
            model_state.set(next_model);
        });
    }
}

#[allow(non_snake_case)]
#[composable]
pub(crate) fn LazyScrollbarRail(
    list_state: cranpose_foundation::lazy::LazyListState,
    model_state: cranpose_core::MutableState<LazyScrollbarModel>,
    semantics_tag: &'static str,
    style: LazyScrollbarStyle,
) {
    let model = model_state.get();
    let thumb_fraction = model.thumb_fraction;
    let scroll_fraction = model.scroll_fraction;

    UiBox(
        Modifier::empty()
            .semantics(|config: &mut SemanticsConfiguration| {
                config.content_description = Some(semantics_tag.to_string());
            })
            .width(style.rail_width)
            .fill_max_height()
            .background(style.rail_color)
            .draw_behind(move |scope| {
                let (thumb_h, thumb_y) = compute_scrollbar_metrics(
                    scope.size().height,
                    thumb_fraction,
                    scroll_fraction,
                    style.min_thumb_height,
                );
                let x = (style.rail_width - style.thumb_width) * 0.5;
                scope.draw_rect_at(
                    cranpose_ui::Rect {
                        x,
                        y: thumb_y,
                        width: style.thumb_width,
                        height: thumb_h,
                    },
                    Brush::solid(style.thumb_color),
                );
            })
            .pointer_input("lazy_scrollbar_drag", move |scope| async move {
                use cranpose_foundation::{PointerButton, PointerEventKind};
                use instant::Instant;
                use std::time::Duration;

                loop {
                    scope
                        .await_pointer_event_scope(|scope| async move {
                            let mut dragging = false;
                            let mut drag_grab_offset = 0.0f32;
                            let mut last_scroll_apply = Instant::now();
                            let mut last_target: Option<(usize, f32)> = None;

                            loop {
                                let event = scope.await_pointer_event().await;
                                match event.kind {
                                    PointerEventKind::Down => {
                                        let (model, rail_h) =
                                            read_interaction_scrollbar_model(list_state);
                                        let inside_rail = event.position.x >= 0.0
                                            && event.position.x <= style.rail_width
                                            && event.position.y >= 0.0
                                            && event.position.y <= rail_h;
                                        if !inside_rail
                                            || !event.buttons.contains(PointerButton::Primary)
                                        {
                                            continue;
                                        }
                                        let (thumb_h, thumb_y) = compute_scrollbar_metrics(
                                            rail_h,
                                            model.thumb_fraction,
                                            model.scroll_fraction,
                                            style.min_thumb_height,
                                        );
                                        if model.max_item_position > 0.0 {
                                            let y = event.position.y.clamp(0.0, rail_h);
                                            let thumb_range = (rail_h - thumb_h).max(0.0);
                                            let target_thumb_y =
                                                (y - thumb_h * 0.5).clamp(0.0, thumb_range);
                                            let target_scroll_fraction = if thumb_range > 0.0 {
                                                target_thumb_y / thumb_range
                                            } else {
                                                0.0
                                            };
                                            let (target_idx, target_offset) =
                                                scroll_target_for_fraction(
                                                    model,
                                                    target_scroll_fraction,
                                                );
                                            list_state.scroll_to_item(target_idx, target_offset);
                                            dragging = true;
                                            drag_grab_offset = (y - thumb_y).clamp(0.0, thumb_h);
                                            last_scroll_apply = Instant::now();
                                            last_target = Some((target_idx, target_offset));
                                            event.consume();
                                        }
                                    }
                                    PointerEventKind::Move if dragging => {
                                        if last_scroll_apply.elapsed() < Duration::from_millis(50) {
                                            event.consume();
                                            continue;
                                        }
                                        let (model, rail_h) =
                                            read_interaction_scrollbar_model(list_state);
                                        let (thumb_h, _) = compute_scrollbar_metrics(
                                            rail_h,
                                            model.thumb_fraction,
                                            model.scroll_fraction,
                                            style.min_thumb_height,
                                        );
                                        let thumb_range = (rail_h - thumb_h).max(0.0);
                                        let target_thumb_y = (event.position.y.clamp(0.0, rail_h)
                                            - drag_grab_offset)
                                            .clamp(0.0, thumb_range);
                                        let target_scroll_fraction = if thumb_range > 0.0 {
                                            target_thumb_y / thumb_range
                                        } else {
                                            0.0
                                        };
                                        let (target_idx, target_offset) =
                                            scroll_target_for_fraction(
                                                model,
                                                target_scroll_fraction,
                                            );
                                        if model.max_item_position > 0.0 {
                                            if model.total_items > 5_000 {
                                                if let Some((last_idx, last_offset)) = last_target {
                                                    let idx_diff = last_idx.abs_diff(target_idx);
                                                    let offset_diff =
                                                        (last_offset - target_offset).abs();
                                                    if idx_diff < 800
                                                        && offset_diff
                                                            < model.average_item_size * 0.5
                                                    {
                                                        event.consume();
                                                        continue;
                                                    }
                                                }
                                            }
                                            list_state.scroll_to_item(target_idx, target_offset);
                                            last_scroll_apply = Instant::now();
                                            last_target = Some((target_idx, target_offset));
                                        }
                                        event.consume();
                                    }
                                    PointerEventKind::Up | PointerEventKind::Cancel => {
                                        if dragging {
                                            event.consume();
                                        }
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                        })
                        .await;
                }
            }),
        BoxSpec::default(),
        || {},
    );
}

#[allow(non_snake_case)]
#[composable]
pub(crate) fn LazyListWithScrollbar<F>(
    modifier: Modifier,
    list_state: cranpose_foundation::lazy::LazyListState,
    rail_tag: &'static str,
    style: LazyScrollbarStyle,
    content: F,
) where
    F: Fn() + 'static,
{
    let model_state = cranpose_core::useState(LazyScrollbarModel::default);
    let content_ref: Rc<dyn Fn()> = Rc::new(content);

    Row(
        modifier.clip_to_bounds(),
        RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            let content_ref_handle = Rc::clone(&content_ref);
            UiBox(
                Modifier::empty()
                    .weight(1.0)
                    .fill_max_height()
                    .clip_to_bounds(),
                BoxSpec::default(),
                move || {
                    (content_ref_handle)();
                },
            );
            LazyScrollbarModelObserver(list_state, model_state);
            LazyScrollbarRail(list_state, model_state, rail_tag, style);
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrollbar_metrics_handle_small_rail_height() {
        let (thumb_h, thumb_y) = compute_scrollbar_metrics(16.0, 0.04, 1.0, 32.0);
        assert_eq!(thumb_h, 16.0);
        assert_eq!(thumb_y, 0.0);
    }

    #[test]
    fn scrollbar_metrics_clamp_scroll_fraction() {
        let (low_h, low_y) = compute_scrollbar_metrics(100.0, 0.5, -10.0, 32.0);
        let (_high_h, high_y) = compute_scrollbar_metrics(100.0, 0.5, 10.0, 32.0);
        assert_eq!(low_h, 50.0);
        assert_eq!(low_y, 0.0);
        assert_eq!(high_y, 50.0);
    }

    #[test]
    fn scrollbar_model_computes_fraction_from_position() {
        let model = compute_scrollbar_model(100, 200.0, 20.0, 10, 10.0);
        assert_eq!(model.total_items, 100);
        assert!((model.max_item_position - 90.0).abs() < 0.001);
        assert!((model.thumb_fraction - 0.1).abs() < 0.001);
        assert!((model.scroll_fraction - (10.5 / 90.0)).abs() < 0.0001);
    }

    #[test]
    fn average_visible_item_size_prefers_measured_visible_items() {
        let layout = cranpose_foundation::lazy::LazyListLayoutInfo {
            visible_items_info: vec![
                cranpose_foundation::lazy::LazyListItemInfo {
                    index: 0,
                    key: 0,
                    offset: 0.0,
                    size: 20.0,
                },
                cranpose_foundation::lazy::LazyListItemInfo {
                    index: 1,
                    key: 1,
                    offset: 20.0,
                    size: 40.0,
                },
            ],
            ..Default::default()
        };

        let avg = average_visible_item_size(&layout, 100.0);
        assert!((avg - 30.0).abs() < 0.001);
    }

    #[test]
    fn stabilize_scrollbar_model_keeps_thumb_visible_when_scrollable() {
        let model = LazyScrollbarModel {
            total_items: 18,
            average_item_size: 32.0,
            max_item_position: 0.0,
            thumb_fraction: 1.0,
            scroll_fraction: 0.0,
        };

        let stabilized = stabilize_scrollbar_model_for_scrollable_content(model, true, false);
        assert!(stabilized.max_item_position > 0.0);
        assert!(stabilized.thumb_fraction < 1.0);
        assert_eq!(stabilized.scroll_fraction, 0.0);
    }

    #[test]
    fn stabilize_scrollbar_model_preserves_non_scrollable_model() {
        let model = LazyScrollbarModel {
            total_items: 5,
            average_item_size: 40.0,
            max_item_position: 0.0,
            thumb_fraction: 1.0,
            scroll_fraction: 0.0,
        };
        let stabilized = stabilize_scrollbar_model_for_scrollable_content(model, false, false);
        assert_eq!(stabilized, model);
    }

    #[test]
    fn scroll_target_for_fraction_maps_to_item_and_offset() {
        let model = compute_scrollbar_model(100, 200.0, 20.0, 0, 0.0);
        let (idx, off) = scroll_target_for_fraction(model, 0.5);
        assert_eq!(idx, 45);
        assert_eq!(off, 0.0);

        let (idx2, off2) = scroll_target_for_fraction(model, 0.5055556);
        assert_eq!(idx2, 45);
        assert!((off2 - 10.0).abs() < 0.001);
    }

    #[test]
    fn scroll_target_for_fraction_handles_non_scrollable_model() {
        let model = compute_scrollbar_model(3, 500.0, 50.0, 0, 0.0);
        assert_eq!(model.max_item_position, 0.0);
        let (idx, off) = scroll_target_for_fraction(model, 1.0);
        assert_eq!(idx, 0);
        assert_eq!(off, 0.0);
    }
}
