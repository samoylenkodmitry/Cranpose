//! The floating text edit menu: a liquid-glass capsule of actions above the
//! selection / caret, matched against the reference recording
//! (`example/target/text-selection/`).
//!
//! Geometry and material (all measured): a 44 dp glass capsule — weak
//! backdrop blur (content behind stays readable through the body), a whisper
//! of dark tint, a top rim highlight — holding white ~15 sp labels separated
//! by 1 dp hairlines (17 dp tall, centered). When the actions don't fit the
//! window, the overflow lives behind a trailing chevron `›` sitting in a
//! lighter glass disc that fills the capsule's end cap (pressing it flashes
//! the disc white and pages the items).
//!
//! Placement: centered over the selection, clamped to a 20 dp screen margin;
//! the capsule's bottom rides 15 dp above the selection's first line.
//!
//! Motion (from the 120 fps reference): the menu dissolves in ~70 ms the
//! moment a handle drag starts, and rematerializes in place ~250 ms after
//! release with a ~140 ms fade (no scale, no slide).

#![allow(non_snake_case)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::composable;
use crate::modifier::{Color, Modifier};
use crate::text::{measure_text, AnnotatedString, TextStyle, TextUnit};
use crate::widgets::box_widget::{Box, BoxSpec};
use crate::widgets::popup::{local_popup_viewport, Popup};
use crate::widgets::{Row, RowSpec, Text};
use crate::PointerInputScope;
use cranpose_animation::{Animatable, AnimationSpec, AnimationType, Easing};
use cranpose_core::{remember, with_current_composer};
use cranpose_foundation::PointerEventKind;
use cranpose_ui_graphics::{
    liquid_menu_glass_effect, GraphicsLayer, LayerShape, Point, Rect, RoundedCornerShape, Size,
};

/// Capsule height (dp) — the measured 133 px @3x.
pub const MENU_HEIGHT: f32 = 44.0;
/// Minimum gap between the capsule and the window edges.
const MENU_SCREEN_MARGIN: f32 = 20.0;
/// Gap between the capsule bottom and the selection's first line top.
const MENU_GAP_ABOVE_LINE: f32 = 15.0;
/// Horizontal padding on each side of an item label.
const ITEM_PADDING: f32 = 20.0;
/// Hairline separator between labels: 1×17 dp, white at 0.07.
const SEPARATOR_WIDTH: f32 = 1.0;
const SEPARATOR_HEIGHT: f32 = 17.0;
const SEPARATOR_COLOR: Color = Color(1.0, 1.0, 1.0, 0.07);
/// Label color and size.
const MENU_FG: Color = Color(0.96, 0.96, 0.98, 1.0);
const MENU_FONT_SP: f32 = 15.0;
/// The chevron disc: lighter glass filling the end cap.
const DISC_COLOR: Color = Color(1.0, 1.0, 1.0, 0.19);
const DISC_PRESSED_COLOR: Color = Color(1.0, 1.0, 1.0, 0.9);
/// Backdrop blur behind the capsule, dp.
/// The materialize smudge radius; the effect resolves it to ~a fifth as the
/// menu sharpens (the settled reference pill keeps backdrop text readable).
const MENU_BLUR_DP: f32 = 15.0;

/// Motion (measured): ~70 ms dissolve, ~140 ms materialize arriving ~250 ms
/// after the release.
const MENU_DISSOLVE_MS: u64 = 70;
const MENU_MATERIALIZE_MS: u64 = 140;
const MENU_RETURN_DELAY_MS: u64 = 250;

fn menu_text_style() -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.color = Some(MENU_FG);
    style.span_style.font_size = TextUnit::Sp(MENU_FONT_SP);
    style
}

/// One tappable action.
#[derive(Clone)]
pub struct TextMenuItem {
    pub label: String,
    pub action: Rc<dyn Fn()>,
}

impl TextMenuItem {
    pub fn new(label: impl Into<String>, action: impl Fn() + 'static) -> Self {
        Self {
            label: label.into(),
            action: Rc::new(action),
        }
    }
}

impl PartialEq for TextMenuItem {
    fn eq(&self, other: &Self) -> bool {
        // Composable memoization identity: the label plus the action's
        // allocation. Freshly captured closures compare unequal, which is
        // correct - they may close over new state.
        self.label == other.label && Rc::ptr_eq(&self.action, &other.action)
    }
}

impl std::fmt::Debug for TextMenuItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextMenuItem")
            .field("label", &self.label)
            .finish()
    }
}

/// Builds the consuming tap gesture for a menu button: it swallows the press,
/// any moves, and the release, and fires `action` when the finger lifts after
/// a press that started on this button. Every event is consumed so the tap
/// can never fall through to the text field beneath the overlay. Keyed by the
/// button label so recomposition reuses the running gesture task.
pub(crate) fn menu_item_pointer_input(label: &str, action: Rc<dyn Fn()>) -> Modifier {
    let key = label.to_string();
    Modifier::empty().pointer_input(key, move |scope: PointerInputScope| {
        let action = Rc::clone(&action);
        async move {
            scope
                .await_pointer_event_scope(|await_scope| async move {
                    // Only a release that follows a press *on this button* runs
                    // the action; a stray release without a press is ignored. The
                    // Down capture keeps the whole gesture on the button, so the
                    // field never sees it.
                    let mut pressed = false;
                    loop {
                        let event = await_scope.await_pointer_event().await;
                        match event.kind {
                            PointerEventKind::Down => {
                                pressed = true;
                                event.consume();
                            }
                            PointerEventKind::Move => {
                                event.consume();
                            }
                            PointerEventKind::Up => {
                                if pressed {
                                    action();
                                }
                                pressed = false;
                                event.consume();
                            }
                            PointerEventKind::Cancel => {
                                pressed = false;
                                event.consume();
                            }
                            _ => {}
                        }
                    }
                })
                .await;
        }
    })
}

/// The chevron disc's press-flash + page gesture: flashes the disc white
/// while held (the reference press feedback) and advances the page on lift.
fn disc_pointer_input(
    pressed: Rc<Cell<bool>>,
    on_tap: Rc<dyn Fn()>,
    invalidate: Rc<dyn Fn()>,
) -> Modifier {
    Modifier::empty().pointer_input("menu-overflow-disc", move |scope: PointerInputScope| {
        let pressed = Rc::clone(&pressed);
        let on_tap = Rc::clone(&on_tap);
        let invalidate = Rc::clone(&invalidate);
        async move {
            scope
                .await_pointer_event_scope(|await_scope| async move {
                    let mut down = false;
                    loop {
                        let event = await_scope.await_pointer_event().await;
                        match event.kind {
                            PointerEventKind::Down => {
                                down = true;
                                pressed.set(true);
                                invalidate();
                                event.consume();
                            }
                            PointerEventKind::Move => {
                                event.consume();
                            }
                            PointerEventKind::Up => {
                                if down {
                                    on_tap();
                                }
                                down = false;
                                pressed.set(false);
                                invalidate();
                                event.consume();
                            }
                            PointerEventKind::Cancel => {
                                down = false;
                                pressed.set(false);
                                invalidate();
                                event.consume();
                            }
                            _ => {}
                        }
                    }
                })
                .await;
        }
    })
}

/// Measured width of one label cell (padding + text + padding), dp.
fn item_width(label: &str, style: &TextStyle) -> f32 {
    measure_text(&AnnotatedString::from(label), style).width + 2.0 * ITEM_PADDING
}

/// Splits `items` into pages that fit `max_width`, appending the chevron
/// disc's width to any page that is followed by more items. Every page holds
/// at least one item.
fn paginate(items: &[TextMenuItem], style: &TextStyle, max_width: f32) -> Vec<Vec<usize>> {
    let widths: Vec<f32> = items
        .iter()
        .map(|item| item_width(&item.label, style))
        .collect();
    let total: f32 =
        widths.iter().sum::<f32>() + SEPARATOR_WIDTH * items.len().saturating_sub(1) as f32;
    if total <= max_width || items.len() <= 1 {
        return vec![(0..items.len()).collect()];
    }
    // Overflow: every page reserves the disc (the chevron also pages BACK
    // from the last page, wrapping — it is always present once paging).
    let disc = MENU_HEIGHT;
    let mut pages: Vec<Vec<usize>> = Vec::new();
    let mut page: Vec<usize> = Vec::new();
    let mut used = disc;
    for (index, width) in widths.iter().enumerate() {
        let extra = width
            + if page.is_empty() {
                0.0
            } else {
                SEPARATOR_WIDTH
            };
        if !page.is_empty() && used + extra > max_width {
            pages.push(std::mem::take(&mut page));
            used = disc;
        }
        used += width
            + if page.is_empty() {
                0.0
            } else {
                SEPARATOR_WIDTH
            };
        page.push(index);
    }
    if !page.is_empty() {
        pages.push(page);
    }
    pages
}

/// Animated visibility state for the menu (dissolve fast, rematerialize
/// after the measured delay).
struct MenuMotion {
    progress: RefCell<Animatable<f32>>,
    was_visible: Cell<bool>,
    page: Cell<usize>,
    disc_pressed: Rc<Cell<bool>>,
}

/// The liquid-glass text edit menu.
///
/// * `center_x` — window-space x to center the capsule on (the selection /
///   caret center); clamped to the screen margin.
/// * `line_top_y` — window-space top of the selection's first line; the
///   capsule bottom rides [`MENU_GAP_ABOVE_LINE`] above it.
/// * `visible` — false while a handle drag is in flight; the menu dissolves
///   and rematerializes per the measured timings (it stays mounted while
///   fading).
/// * `items` — the actions.
#[composable]
pub fn LiquidTextMenu(center_x: f32, line_top_y: f32, visible: bool, items: Vec<TextMenuItem>) {
    let motion = remember(|| {
        let runtime = with_current_composer(|composer| composer.runtime_handle());
        Rc::new(MenuMotion {
            progress: RefCell::new(Animatable::new(0.0, runtime)),
            was_visible: Cell::new(false),
            page: Cell::new(0),
            disc_pressed: Rc::new(Cell::new(false)),
        })
    })
    .with(Rc::clone);

    if visible != motion.was_visible.get() {
        motion.was_visible.set(visible);
        let mut progress = motion.progress.borrow_mut();
        if visible {
            progress.animateTo(
                1.0,
                AnimationType::Tween(
                    AnimationSpec::tween(MENU_MATERIALIZE_MS, Easing::EaseOut)
                        .with_delay(MENU_RETURN_DELAY_MS),
                ),
            );
        } else {
            progress.animateTo(
                0.0,
                AnimationType::Tween(AnimationSpec::tween(MENU_DISSOLVE_MS, Easing::LinearEasing)),
            );
        }
    }
    let progress_state = motion.progress.borrow().state();
    let p = progress_state.value().clamp(0.0, 1.0);
    if !visible && p <= 0.01 {
        return;
    }

    let style = menu_text_style();
    let viewport = local_popup_viewport().current().get();
    let max_width = if viewport.width > 0.0 {
        viewport.width - 2.0 * MENU_SCREEN_MARGIN
    } else {
        f32::INFINITY
    };
    let pages = paginate(&items, &style, max_width);
    let page_index = motion.page.get().min(pages.len() - 1);
    let page = &pages[page_index];
    let has_disc = pages.len() > 1;

    // Capsule width from the measured labels (the same measurer the labels
    // lay out with), for centering + clamping.
    let mut width: f32 = page
        .iter()
        .map(|&i| item_width(&items[i].label, &style))
        .sum();
    width += SEPARATOR_WIDTH * page.len().saturating_sub(1) as f32;
    if has_disc {
        width += MENU_HEIGHT;
    }

    let mut x = center_x - width * 0.5;
    if viewport.width > 0.0 {
        x = x.min(viewport.width - MENU_SCREEN_MARGIN - width);
    }
    x = x.max(MENU_SCREEN_MARGIN);
    let y = line_top_y - MENU_GAP_ABOVE_LINE - MENU_HEIGHT;

    let anchor = Rect {
        x,
        y,
        width: 0.0,
        height: 0.0,
    };
    let density = crate::current_density();
    let page_items: Vec<TextMenuItem> = page.iter().map(|&i| items[i].clone()).collect();
    let disc_pressed = Rc::clone(&motion.disc_pressed);
    let page_count = pages.len();
    let motion_for_disc = Rc::clone(&motion);
    Popup(anchor, Point { x: 0.0, y: 0.0 }, move || {
        let page_items = page_items.clone();
        let disc_pressed = Rc::clone(&disc_pressed);
        let motion = Rc::clone(&motion_for_disc);
        Box(
            Modifier::empty()
                .size(Size {
                    width,
                    height: MENU_HEIGHT,
                })
                // The reference capsule floats on a soft elevation shadow
                // (~a 45px halo). Knocked out of its own silhouette — glass
                // samples the backdrop behind itself and must not refract
                // its own shadow.
                .drop_shadow(
                    LayerShape::Rounded(RoundedCornerShape::uniform(1.0e6)),
                    move |scope| {
                        // A transient BRIGHT bloom while the pill
                        // materializes (the reference's disc flash / glow),
                        // gone at rest — the settled reference shows a flat
                        // baseline right up to the rim, and a dark band is
                        // invisible on the dark card anyway.
                        scope.radius = 16.0;
                        scope.spread = -2.0;
                        scope.offset.y = 0.0;
                        scope.color = Color(1.0, 1.0, 1.0, 0.12 * p * (1.0 - p));
                        scope.cutout = true;
                    },
                )
                .graphics_layer(move || GraphicsLayer {
                    alpha: p,
                    // No material during the return-delay window (p≈0): a
                    // composed backdrop blur ignores layer alpha and would
                    // smear the content behind the pill a quarter-second
                    // before anything fades in — the reference shows nothing
                    // until the fade starts.
                    backdrop_effect: (p > 0.001).then(|| {
                        liquid_menu_glass_effect((width, MENU_HEIGHT), MENU_BLUR_DP * density, p)
                    }),
                    shape: LayerShape::Rounded(RoundedCornerShape::uniform(1.0e6)),
                    clip: true,
                    ..Default::default()
                }),
            BoxSpec::default(),
            move || {
                let page_items = page_items.clone();
                let disc_pressed = Rc::clone(&disc_pressed);
                let motion = Rc::clone(&motion);
                Row(
                    Modifier::empty().size(Size {
                        width,
                        height: MENU_HEIGHT,
                    }),
                    RowSpec::default().vertical_alignment(
                        cranpose_ui_layout::VerticalAlignment::CenterVertically,
                    ),
                    move || {
                        for (index, item) in page_items.iter().enumerate() {
                            if index > 0 {
                                Box(
                                    Modifier::empty()
                                        .size(Size {
                                            width: SEPARATOR_WIDTH,
                                            height: SEPARATOR_HEIGHT,
                                        })
                                        .background(SEPARATOR_COLOR),
                                    BoxSpec::default(),
                                    || {},
                                );
                            }
                            Text(
                                item.label.clone(),
                                Modifier::empty()
                                    // The glyphs sit low in this stack's line
                                    // box; the reference centers them to a
                                    // pixel, so bias 2dp onto the bottom.
                                    .padding_each(ITEM_PADDING, 0.0, ITEM_PADDING, 2.0)
                                    .then(menu_item_pointer_input(
                                        &item.label,
                                        Rc::clone(&item.action),
                                    )),
                                menu_text_style(),
                            );
                        }
                        if page_count > 1 {
                            // The lighter glass disc filling the end cap, with
                            // the paging chevron; flashes white while pressed.
                            let pressed_now = disc_pressed.get();
                            let motion = Rc::clone(&motion);
                            let advance: Rc<dyn Fn()> = Rc::new(move || {
                                motion.page.set((motion.page.get() + 1) % page_count);
                                crate::request_render_invalidation();
                            });
                            let invalidate: Rc<dyn Fn()> =
                                Rc::new(crate::request_render_invalidation);
                            Box(
                                Modifier::empty()
                                    .size(Size {
                                        width: MENU_HEIGHT,
                                        height: MENU_HEIGHT,
                                    })
                                    .background(if pressed_now {
                                        DISC_PRESSED_COLOR
                                    } else {
                                        DISC_COLOR
                                    })
                                    .rounded_corners(MENU_HEIGHT * 0.5)
                                    .then(disc_pointer_input(
                                        Rc::clone(&disc_pressed),
                                        advance,
                                        invalidate,
                                    )),
                                BoxSpec::default()
                                    .content_alignment(cranpose_ui_layout::Alignment::CENTER),
                                || {
                                    Text(
                                        "\u{203A}".to_string(),
                                        Modifier::empty(),
                                        menu_text_style(),
                                    );
                                },
                            );
                        }
                    },
                );
            },
        );
    });
}

/// A floating Copy / Cut / Paste / Select-all menu shown just above the text
/// selection. `center_x` / `line_top_y` anchor it over the selection;
/// `visible` is false while a handle drag is in flight. `can_paste` hides the
/// Paste item when the clipboard is empty. Each action runs against the
/// focused field; the caller is expected to dismiss the menu.
#[allow(clippy::too_many_arguments)]
#[composable]
pub fn TextSelectionMenu(
    center_x: f32,
    line_top_y: f32,
    visible: bool,
    can_paste: bool,
    on_copy: impl Fn() + 'static,
    on_cut: impl Fn() + 'static,
    on_paste: impl Fn() + 'static,
    on_select_all: impl Fn() + 'static,
) {
    let mut items = vec![
        TextMenuItem::new("Copy", on_copy),
        TextMenuItem::new("Cut", on_cut),
    ];
    if can_paste {
        items.push(TextMenuItem::new("Paste", on_paste));
    }
    items.push(TextMenuItem::new("Select all", on_select_all));
    LiquidTextMenu(center_x, line_top_y, visible, items);
}

/// A floating Paste / Select all / Undo / Redo menu shown near the collapsed
/// caret. Opened by tapping the cursor handle (there is no selection to
/// Copy/Cut, so this offers the caret-relevant actions instead).
///
/// `can_paste` hides Paste when the clipboard is empty; `can_undo`/`can_redo`
/// hide those items when the field's history has nothing to undo/redo.
#[allow(clippy::too_many_arguments)]
#[composable]
pub fn CaretActionMenu(
    center_x: f32,
    line_top_y: f32,
    visible: bool,
    can_paste: bool,
    can_undo: bool,
    can_redo: bool,
    on_paste: impl Fn() + 'static,
    on_select_all: impl Fn() + 'static,
    on_undo: impl Fn() + 'static,
    on_redo: impl Fn() + 'static,
) {
    let mut items = Vec::new();
    if can_paste {
        items.push(TextMenuItem::new("Paste", on_paste));
    }
    items.push(TextMenuItem::new("Select all", on_select_all));
    if can_undo {
        items.push(TextMenuItem::new("Undo", on_undo));
    }
    if can_redo {
        items.push(TextMenuItem::new("Redo", on_redo));
    }
    LiquidTextMenu(center_x, line_top_y, visible, items);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modifier::{collect_slices_from_modifier, ModifierNodeSlices};
    use cranpose_foundation::PointerEvent;
    use cranpose_ui_graphics::Point;

    /// Collects the button's live pointer-input handler. Returns the owning
    /// [`ModifierNodeSlices`] too: it keeps the attached node (and its running
    /// coroutine) alive — dropping it would cancel the gesture and swallow the
    /// events.
    fn button_handler(modifier: &Modifier) -> (Rc<dyn Fn(PointerEvent)>, ModifierNodeSlices) {
        let slices = collect_slices_from_modifier(modifier);
        assert_eq!(
            slices.pointer_inputs().len(),
            1,
            "menu button must install exactly one pointer-input gesture"
        );
        let handler = slices.pointer_inputs()[0].clone();
        (handler, slices)
    }

    fn down(x: f32, y: f32) -> PointerEvent {
        PointerEvent::new(PointerEventKind::Down, Point { x, y }, Point { x, y })
    }
    fn up(x: f32, y: f32) -> PointerEvent {
        PointerEvent::new(PointerEventKind::Up, Point { x, y }, Point { x, y })
    }

    /// Bug 7: a tap (press then release) on a menu button consumes BOTH the
    /// press and the release — so the tap can never fall through to the text
    /// field below (which would collapse the selection) — and runs the action on
    /// release.
    #[test]
    fn menu_button_consumes_the_tap_and_runs_the_action() {
        let _app_context = crate::render_state::app_context_test_scope();
        let ran = Rc::new(Cell::new(false));
        let action: Rc<dyn Fn()> = {
            let ran = Rc::clone(&ran);
            Rc::new(move || ran.set(true))
        };
        let modifier = menu_item_pointer_input("Copy", action);
        let (handler, _slices) = button_handler(&modifier);

        let press = down(5.0, 5.0);
        handler(press.clone());
        assert!(
            press.is_consumed(),
            "the press must be consumed so it never reaches the field and collapses the selection"
        );
        assert!(!ran.get(), "the action fires on release, not on press");

        let release = up(6.0, 6.0);
        handler(release.clone());
        assert!(release.is_consumed(), "the release must be consumed too");
        assert!(
            ran.get(),
            "releasing after a press on the button runs the action"
        );
    }

    /// A stray release with no preceding press on this button is still consumed
    /// (never reaches the field) but does not run the action.
    #[test]
    fn menu_button_release_without_press_is_consumed_but_inert() {
        let _app_context = crate::render_state::app_context_test_scope();
        let ran = Rc::new(Cell::new(false));
        let action: Rc<dyn Fn()> = {
            let ran = Rc::clone(&ran);
            Rc::new(move || ran.set(true))
        };
        let modifier = menu_item_pointer_input("Cut", action);
        let (handler, _slices) = button_handler(&modifier);

        let release = up(5.0, 5.0);
        handler(release.clone());
        assert!(
            release.is_consumed(),
            "a release on the menu is consumed so it never hits the field"
        );
        assert!(!ran.get(), "a release with no matching press must not act");
    }

    /// The measured layout constants: a 44 dp capsule with 20 dp label
    /// padding and 1×17 dp separators.
    #[test]
    fn menu_geometry_matches_the_reference() {
        assert_eq!(MENU_HEIGHT, 44.0);
        assert_eq!(ITEM_PADDING, 20.0);
        assert_eq!(SEPARATOR_WIDTH, 1.0);
        assert_eq!(SEPARATOR_HEIGHT, 17.0);
        assert_eq!(MENU_GAP_ABOVE_LINE, 15.0);
        assert_eq!(MENU_SCREEN_MARGIN, 20.0);
    }

    /// Pagination: everything fits on one page when there is room; a narrow
    /// window splits into pages, each reserving the chevron disc, and every
    /// page keeps at least one item.
    #[test]
    fn pagination_reserves_the_disc_only_when_overflowing() {
        let _app_context = crate::render_state::app_context_test_scope();
        let style = menu_text_style();
        let items: Vec<TextMenuItem> = ["Copy", "Cut", "Paste", "Select all"]
            .iter()
            .map(|label| TextMenuItem::new(*label, || {}))
            .collect();

        let one = paginate(&items, &style, f32::INFINITY);
        assert_eq!(one.len(), 1, "everything fits on one page");
        assert_eq!(one[0].len(), 4);

        let total: f32 = items
            .iter()
            .map(|i| item_width(&i.label, &style))
            .sum::<f32>()
            + 3.0 * SEPARATOR_WIDTH;
        let narrow = paginate(&items, &style, total * 0.55);
        assert!(narrow.len() > 1, "a narrow window must page");
        assert!(narrow.iter().all(|page| !page.is_empty()));
        let all: Vec<usize> = narrow.iter().flatten().copied().collect();
        assert_eq!(all, vec![0, 1, 2, 3], "pages cover every item in order");
    }
}
