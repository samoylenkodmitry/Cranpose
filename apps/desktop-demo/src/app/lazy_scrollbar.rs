use std::{cell::RefCell, rc::Rc};

use cranpose_foundation::SemanticsConfiguration;
use cranpose_ui::{
    composable, Box as UiBox, BoxSpec, Brush, Color, LinearArrangement, Modifier, Row, RowSpec,
};

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

#[composable]
pub(crate) fn LazyScrollbarRail(
    list_state: cranpose_foundation::lazy::LazyListState,
    semantics_tag: &'static str,
    style: LazyScrollbarStyle,
) {
    let (model, _) = read_interaction_scrollbar_model(list_state);
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
                use std::time::Duration;

                use cranpose_foundation::{PointerButton, PointerEventKind};
                use instant::Instant;

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
    let content_cell = cranpose_core::remember(|| Rc::new(RefCell::new(None::<Rc<dyn Fn()>>)))
        .with(|cell| cell.clone());
    *content_cell.borrow_mut() = Some(Rc::new(content));

    Row(
        modifier.clip_to_bounds(),
        RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            let content_cell_handle = Rc::clone(&content_cell);
            UiBox(
                Modifier::empty()
                    .weight(1.0)
                    .fill_max_height()
                    .clip_to_bounds(),
                BoxSpec::default(),
                move || {
                    if let Some(content) = content_cell_handle.borrow().as_ref() {
                        (content)();
                    }
                },
            );
            LazyScrollbarRail(list_state, rail_tag, style);
        },
    );
}

#[cfg(test)]
#[path = "tests/lazy_scrollbar_tests.rs"]
mod tests;
