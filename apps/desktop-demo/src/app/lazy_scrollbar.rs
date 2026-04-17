use cranpose_foundation::SemanticsConfiguration;
use cranpose_ui::{
    composable, Box as UiBox, BoxSpec, Brush, Color, LinearArrangement, Modifier, Row, RowSpec,
};
use std::cell::RefCell;
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
mod tests {
    use super::*;
    use cranpose_core::{location_key, Applier, Composition, MemoryApplier, MutableState, NodeId};
    use cranpose_foundation::lazy::{remember_lazy_list_state, LazyListScope};
    use cranpose_ui::{
        BoxWithConstraints, Column, ColumnSpec, HeadlessRenderer, LayoutEngine, LazyColumn,
        LazyColumnSpec, Modifier, RenderOp, Size, Text, TextStyle,
    };
    use std::cell::RefCell;

    thread_local! {
        static CAPTURED_SCROLLBAR_LIST_STATE: RefCell<Option<cranpose_foundation::lazy::LazyListState>> = const { RefCell::new(None) };
        static CAPTURED_SCROLLBAR_LIST_NODE_ID: RefCell<Option<NodeId>> = const { RefCell::new(None) };
    }

    fn test_style() -> LazyScrollbarStyle {
        LazyScrollbarStyle {
            rail_width: 8.0,
            thumb_width: 6.0,
            min_thumb_height: 16.0,
            rail_color: Color(0.2, 0.2, 0.2, 1.0),
            thumb_color: Color(0.8, 0.8, 0.8, 1.0),
        }
    }

    fn render_texts(composition: &mut Composition<MemoryApplier>, root: NodeId) -> Vec<String> {
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        let layout = applier
            .compute_layout(
                root,
                Size {
                    width: 200.0,
                    height: 200.0,
                },
            )
            .expect("layout");
        applier.clear_runtime_handle();
        let renderer = HeadlessRenderer::new();
        let scene = renderer.render(&layout);
        scene
            .operations()
            .iter()
            .filter_map(|op| match op {
                RenderOp::Text { value, .. } => Some(value.clone()),
                _ => None,
            })
            .collect()
    }

    fn measure_root(composition: &mut Composition<MemoryApplier>, root: NodeId, size: Size) {
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        let _ = applier.compute_layout(root, size).expect("layout");
        applier.clear_runtime_handle();
    }

    fn node_generation(composition: &mut Composition<MemoryApplier>, node_id: NodeId) -> u32 {
        composition.applier_mut().node_generation(node_id)
    }

    fn settle_recomposition(
        composition: &mut Composition<MemoryApplier>,
        root: NodeId,
        size: Size,
    ) {
        measure_root(composition, root, size);
        while composition
            .process_invalid_scopes()
            .expect("recomposition should succeed")
        {
            measure_root(composition, root, size);
        }
        measure_root(composition, root, size);
    }

    #[composable]
    fn dynamic_stories_pane(
        modifier: Modifier,
        list_state: cranpose_foundation::lazy::LazyListState,
        loaded_count: MutableState<usize>,
    ) {
        Column(modifier, ColumnSpec::default(), move || {
            Text("Top stories", Modifier::empty(), TextStyle::default());
            Text(
                format!("{} loaded of 60", loaded_count.value()),
                Modifier::empty(),
                TextStyle::default(),
            );
            LazyListWithScrollbar(
                Modifier::empty().fill_max_width().weight(1.0),
                list_state,
                "StoriesRail",
                test_style(),
                move || {
                    let node_id = LazyColumn(
                        Modifier::empty().fill_max_size(),
                        list_state,
                        LazyColumnSpec::default(),
                        |scope| {
                            scope.items(
                                40,
                                None::<fn(usize) -> u64>,
                                None::<fn(usize) -> u64>,
                                |index| {
                                    Text(
                                        format!("Item {}", index),
                                        Modifier::empty(),
                                        TextStyle::default(),
                                    );
                                },
                            );
                        },
                    );
                    CAPTURED_SCROLLBAR_LIST_STATE.with(|slot| {
                        *slot.borrow_mut() = Some(list_state);
                    });
                    CAPTURED_SCROLLBAR_LIST_NODE_ID.with(|slot| {
                        *slot.borrow_mut() = Some(node_id);
                    });
                },
            );
        });
    }

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

    #[test]
    fn lazy_list_with_scrollbar_restores_hidden_branch() {
        let mut composition = Composition::new(MemoryApplier::new());
        let runtime = composition.runtime_handle();
        let show_thread = MutableState::with_runtime(false, runtime.clone());

        composition
            .render(location_key(file!(), line!(), column!()), || {
                let list_state = remember_lazy_list_state();
                BoxWithConstraints(Modifier::empty().fill_max_size(), move |_scope| {
                    Column(
                        Modifier::empty().fill_max_size(),
                        ColumnSpec::default(),
                        move || {
                            Text("Header", Modifier::empty(), TextStyle::default());
                            if show_thread.value() {
                                Text("Thread", Modifier::empty(), TextStyle::default());
                            } else {
                                LazyListWithScrollbar(
                                    Modifier::empty().fill_max_width().weight(1.0),
                                    list_state,
                                    "TestRail",
                                    test_style(),
                                    move || {
                                        LazyColumn(
                                            Modifier::empty().fill_max_size(),
                                            list_state,
                                            LazyColumnSpec::default(),
                                            |scope| {
                                                scope.item(Some(0), None, || {
                                                    Text(
                                                        "Stories",
                                                        Modifier::empty(),
                                                        TextStyle::default(),
                                                    );
                                                });
                                            },
                                        );
                                    },
                                );
                            }
                        },
                    );
                });
            })
            .expect("initial render");

        let root = composition.root().expect("root");
        let texts = render_texts(&mut composition, root);
        assert!(texts.iter().any(|text| text == "Stories"));

        show_thread.set_value(true);
        composition
            .process_invalid_scopes()
            .expect("switch to thread branch");
        let texts = render_texts(&mut composition, root);
        assert!(texts.iter().any(|text| text == "Thread"));
        assert!(!texts.iter().any(|text| text == "Stories"));

        show_thread.set_value(false);
        composition
            .process_invalid_scopes()
            .expect("restore stories branch");
        let texts = render_texts(&mut composition, root);
        assert!(
            texts.iter().any(|text| text == "Stories"),
            "restored scrollbar branch should render again, got {texts:?}",
        );
        assert!(!texts.iter().any(|text| text == "Thread"));
    }

    #[test]
    fn lazy_list_with_scrollbar_restores_keyed_items_branch() {
        let mut composition = Composition::new(MemoryApplier::new());
        let runtime = composition.runtime_handle();
        let show_thread = MutableState::with_runtime(false, runtime.clone());

        composition
            .render(location_key(file!(), line!(), column!()), || {
                let list_state = remember_lazy_list_state();
                BoxWithConstraints(Modifier::empty().fill_max_size(), move |_scope| {
                    Column(
                        Modifier::empty().fill_max_size(),
                        ColumnSpec::default(),
                        move || {
                            Text("Header", Modifier::empty(), TextStyle::default());
                            if show_thread.value() {
                                Text("Thread", Modifier::empty(), TextStyle::default());
                            } else {
                                LazyListWithScrollbar(
                                    Modifier::empty().fill_max_width().weight(1.0),
                                    list_state,
                                    "TestRail",
                                    test_style(),
                                    move || {
                                        let stories = Rc::new(vec![
                                            "Story 1".to_string(),
                                            "Story 2".to_string(),
                                            "Story 3".to_string(),
                                        ]);
                                        LazyColumn(
                                            Modifier::empty().fill_max_size(),
                                            list_state,
                                            LazyColumnSpec::default(),
                                            {
                                                let stories = Rc::clone(&stories);
                                                move |scope| {
                                                    scope.items(
                                                        stories.len(),
                                                        Some(|index| index as u64),
                                                        None::<fn(usize) -> u64>,
                                                        {
                                                            let stories = Rc::clone(&stories);
                                                            move |index| {
                                                                Text(
                                                                    stories[index].clone(),
                                                                    Modifier::empty(),
                                                                    TextStyle::default(),
                                                                );
                                                            }
                                                        },
                                                    );
                                                    scope.item(Some(9999), None, || {
                                                        Text(
                                                            "Footer",
                                                            Modifier::empty(),
                                                            TextStyle::default(),
                                                        );
                                                    });
                                                }
                                            },
                                        );
                                    },
                                );
                            }
                        },
                    );
                });
            })
            .expect("initial render");

        let root = composition.root().expect("root");
        let texts = render_texts(&mut composition, root);
        assert!(texts.iter().any(|text| text == "Story 1"));

        show_thread.set_value(true);
        composition
            .process_invalid_scopes()
            .expect("switch to thread branch");
        let texts = render_texts(&mut composition, root);
        assert!(texts.iter().any(|text| text == "Thread"));

        show_thread.set_value(false);
        composition
            .process_invalid_scopes()
            .expect("restore stories branch");
        let texts = render_texts(&mut composition, root);
        assert!(
            texts.iter().any(|text| text == "Story 1"),
            "restored keyed scrollbar branch should render again, got {texts:?}",
        );
        assert!(texts.iter().any(|text| text == "Footer"));
    }

    #[test]
    fn lazy_list_with_scrollbar_restores_after_keyed_branch_cycle() {
        let mut composition = Composition::new(MemoryApplier::new());
        let runtime = composition.runtime_handle();
        let show_thread = MutableState::with_runtime(false, runtime.clone());

        composition
            .render(location_key(file!(), line!(), column!()), || {
                let stories_list_state = remember_lazy_list_state();
                BoxWithConstraints(Modifier::empty().fill_max_size(), move |_scope| {
                    Column(
                        Modifier::empty().fill_max_size(),
                        ColumnSpec::default(),
                        move || {
                            Text("Header", Modifier::empty(), TextStyle::default());
                            if show_thread.value() {
                                cranpose_core::with_key(&1u64, move || {
                                    let comments_list_state = remember_lazy_list_state();
                                    LazyListWithScrollbar(
                                        Modifier::empty().fill_max_size(),
                                        comments_list_state,
                                        "CommentsRail",
                                        test_style(),
                                        move || {
                                            LazyColumn(
                                                Modifier::empty().fill_max_size(),
                                                comments_list_state,
                                                LazyColumnSpec::default(),
                                                |scope| {
                                                    scope.item(Some(0), None, || {
                                                        Text(
                                                            "Thread",
                                                            Modifier::empty(),
                                                            TextStyle::default(),
                                                        );
                                                    });
                                                },
                                            );
                                        },
                                    );
                                });
                            } else {
                                LazyListWithScrollbar(
                                    Modifier::empty().fill_max_width().weight(1.0),
                                    stories_list_state,
                                    "StoriesRail",
                                    test_style(),
                                    move || {
                                        LazyColumn(
                                            Modifier::empty().fill_max_size(),
                                            stories_list_state,
                                            LazyColumnSpec::default(),
                                            |scope| {
                                                scope.item(Some(0), None, || {
                                                    Text(
                                                        "Stories",
                                                        Modifier::empty(),
                                                        TextStyle::default(),
                                                    );
                                                });
                                            },
                                        );
                                    },
                                );
                            }
                        },
                    );
                });
            })
            .expect("initial render");

        let root = composition.root().expect("root");
        let texts = render_texts(&mut composition, root);
        assert!(texts.iter().any(|text| text == "Stories"));

        show_thread.set_value(true);
        composition
            .process_invalid_scopes()
            .expect("switch to keyed thread branch");
        let texts = render_texts(&mut composition, root);
        assert!(texts.iter().any(|text| text == "Thread"));
        assert!(!texts.iter().any(|text| text == "Stories"));

        show_thread.set_value(false);
        composition
            .process_invalid_scopes()
            .expect("restore stories branch after keyed thread");
        let texts = render_texts(&mut composition, root);
        assert!(
            texts.iter().any(|text| text == "Stories"),
            "stories branch should restore after keyed thread cycle, got {texts:?}",
        );
        assert!(!texts.iter().any(|text| text == "Thread"));
    }

    #[test]
    fn lazy_list_with_scrollbar_restored_branch_keeps_host_during_scroll_and_status_recompose() {
        let mut composition = Composition::new(MemoryApplier::new());
        let runtime = composition.runtime_handle();
        let show_thread = MutableState::with_runtime(false, runtime.clone());
        let loaded_count = MutableState::with_runtime(20usize, runtime.clone());

        CAPTURED_SCROLLBAR_LIST_STATE.with(|slot| *slot.borrow_mut() = None);
        CAPTURED_SCROLLBAR_LIST_NODE_ID.with(|slot| *slot.borrow_mut() = None);

        composition
            .render(location_key(file!(), line!(), column!()), || {
                let list_state = remember_lazy_list_state();
                BoxWithConstraints(Modifier::empty().fill_max_size(), move |_scope| {
                    Column(
                        Modifier::empty().fill_max_size(),
                        ColumnSpec::default(),
                        move || {
                            Text("Header", Modifier::empty(), TextStyle::default());
                            let branch_key = show_thread.value();
                            cranpose_core::with_key(&branch_key, move || {
                                if branch_key {
                                    Text("Thread", Modifier::empty(), TextStyle::default());
                                } else {
                                    dynamic_stories_pane(
                                        Modifier::empty().fill_max_width().weight(1.0),
                                        list_state,
                                        loaded_count,
                                    );
                                }
                            });
                        },
                    );
                });
            })
            .expect("initial render");

        let root = composition.root().expect("root");
        let size = Size {
            width: 260.0,
            height: 360.0,
        };
        settle_recomposition(&mut composition, root, size);

        show_thread.set_value(true);
        settle_recomposition(&mut composition, root, size);

        show_thread.set_value(false);
        settle_recomposition(&mut composition, root, size);

        let restored_node_id = CAPTURED_SCROLLBAR_LIST_NODE_ID
            .with(|slot| (*slot.borrow()).expect("restored list node should exist"));
        let restored_generation = node_generation(&mut composition, restored_node_id);
        let list_state = CAPTURED_SCROLLBAR_LIST_STATE
            .with(|slot| (*slot.borrow()).expect("restored list state should exist"));

        list_state.dispatch_scroll_delta(-120.0);
        loaded_count.set_value(40);
        settle_recomposition(&mut composition, root, size);

        let after_node_id = CAPTURED_SCROLLBAR_LIST_NODE_ID
            .with(|slot| (*slot.borrow()).expect("list node should exist after scroll"));
        let after_generation = node_generation(&mut composition, after_node_id);

        assert_eq!(
            (after_node_id, after_generation),
            (restored_node_id, restored_generation),
            "restored scrollbar lazy-list host changed after scroll + status recomposition; restored=({restored_node_id}, {restored_generation}) after=({after_node_id}, {after_generation})",
        );
    }

    #[test]
    fn lazy_list_with_scrollbar_restored_branch_keeps_host_during_repeated_scrolls() {
        let mut composition = Composition::new(MemoryApplier::new());
        let runtime = composition.runtime_handle();
        let show_thread = MutableState::with_runtime(false, runtime.clone());
        let loaded_count = MutableState::with_runtime(20usize, runtime.clone());

        CAPTURED_SCROLLBAR_LIST_STATE.with(|slot| *slot.borrow_mut() = None);
        CAPTURED_SCROLLBAR_LIST_NODE_ID.with(|slot| *slot.borrow_mut() = None);

        composition
            .render(location_key(file!(), line!(), column!()), || {
                let list_state = remember_lazy_list_state();
                BoxWithConstraints(Modifier::empty().fill_max_size(), move |_scope| {
                    Column(
                        Modifier::empty().fill_max_size(),
                        ColumnSpec::default(),
                        move || {
                            Text("Header", Modifier::empty(), TextStyle::default());
                            let branch_key = show_thread.value();
                            cranpose_core::with_key(&branch_key, move || {
                                if branch_key {
                                    Text("Thread", Modifier::empty(), TextStyle::default());
                                } else {
                                    dynamic_stories_pane(
                                        Modifier::empty().fill_max_width().weight(1.0),
                                        list_state,
                                        loaded_count,
                                    );
                                }
                            });
                        },
                    );
                });
            })
            .expect("initial render");

        let root = composition.root().expect("root");
        let size = Size {
            width: 260.0,
            height: 360.0,
        };
        settle_recomposition(&mut composition, root, size);

        show_thread.set_value(true);
        settle_recomposition(&mut composition, root, size);

        show_thread.set_value(false);
        settle_recomposition(&mut composition, root, size);

        let restored_node_id = CAPTURED_SCROLLBAR_LIST_NODE_ID
            .with(|slot| (*slot.borrow()).expect("restored list node should exist"));
        let restored_generation = node_generation(&mut composition, restored_node_id);
        let list_state = CAPTURED_SCROLLBAR_LIST_STATE
            .with(|slot| (*slot.borrow()).expect("restored list state should exist"));

        let mut seen = Vec::new();
        for _ in 0..6 {
            list_state.dispatch_scroll_delta(-120.0);
            settle_recomposition(&mut composition, root, size);

            let node_id = CAPTURED_SCROLLBAR_LIST_NODE_ID
                .with(|slot| (*slot.borrow()).expect("list node should exist after scroll"));
            let generation = node_generation(&mut composition, node_id);
            seen.push((node_id, generation));
        }

        assert_eq!(
            seen,
            vec![(restored_node_id, restored_generation); seen.len()],
            "restored scrollbar lazy-list host changed across repeated scrolls; restored=({restored_node_id}, {restored_generation}) seen={seen:?}",
        );
    }
}
