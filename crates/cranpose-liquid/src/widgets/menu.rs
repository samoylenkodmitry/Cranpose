//! A popup menu that morphs out of its anchor: the glass bubble springs from
//! the anchor corner while its items fade in (the WWDC "Show" menu).

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{MutableState, SideEffect, mutableStateOf, remember, rememberMutableStateOf};
use cranpose_foundation::PointerId;
use cranpose_macros::composable;
use cranpose_ui::{
    Modifier, PointerEventKind, PointerInputScope, PressInteractionPress, SemanticsWidgetRole,
    Size, rememberMutableInteractionSource,
    text::{FontWeight, SpanStyle, TextStyle, TextUnit},
    widgets::{Box, BoxSpec, Column, ColumnSpec, PopupDismissableWhen, Row, RowSpec, Text},
};
use cranpose_ui_graphics::{Brush, Color, CornerRadii, GraphicsLayer, Point, Rect, RenderEffect};
use cranpose_ui_layout::VerticalAlignment;

use crate::{
    material::{Glass, GlassDynamics, GlassMorph, GlassShadow, LiquidModifierExt, LiquidShape},
    theme::{liquid_colors, liquid_typography},
    widgets::content_scope::ScopeContent,
};

/// One menu entry.
#[derive(Clone, Debug, PartialEq)]
pub struct LiquidMenuItem {
    pub label: String,
    /// Optional leading icon (24×24 path data).
    pub icon: Option<&'static str>,
    /// Draws the leading checkmark (selected state).
    pub checked: bool,
    /// Destructive styling.
    pub destructive: bool,
    /// Starts a new visual section (full-width hairline above).
    pub section_start: bool,
    /// A non-interactive gray section header ("Show").
    pub header: bool,
    /// Selecting this row keeps the menu open (an accordion row: the caller
    /// swaps `items` and the surface morphs to the new size in place).
    pub keeps_open: bool,
    /// Optional gray second line under the label (the reference sort/filter
    /// rows describe their current state: "Sections. Unread messages on
    /// top."). Accordion rows draw a trailing chevron when present.
    pub subtitle: Option<String>,
}

impl LiquidMenuItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            checked: false,
            destructive: false,
            section_start: false,
            header: false,
            keeps_open: false,
            subtitle: None,
        }
    }

    /// A gray, non-interactive section header row.
    pub fn header(label: impl Into<String>) -> Self {
        Self {
            header: true,
            ..Self::new(label)
        }
    }

    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }

    pub fn section_start(mut self) -> Self {
        self.section_start = true;
        self
    }

    /// Marks an accordion row: selecting it keeps the menu open while the
    /// caller swaps the item list (the surface morphs to the new size).
    pub fn keeps_open(mut self) -> Self {
        self.keeps_open = true;
        self
    }

    /// Gray descriptive second line under the label.
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }
}

/// A neighboring glass icon control whose volume and foreground are absorbed
/// by an opening menu surface.
#[derive(Clone, Debug, PartialEq)]
pub struct LiquidMenuAbsorbedSource {
    pub rect: Rect,
    pub spec: crate::widgets::GlassButtonSpec,
    pub diameter: f32,
    pub icon_path: &'static str,
}

/// Visual configuration for an anchored liquid dropdown.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LiquidDropdownMenuSpec {
    pub menu: LiquidMenuSpec,
    pub absorbed: Vec<LiquidMenuAbsorbedSource>,
}

impl LiquidDropdownMenuSpec {
    pub fn menu(mut self, menu: LiquidMenuSpec) -> Self {
        self.menu = menu;
        self
    }

    pub fn absorbed(mut self, absorbed: Vec<LiquidMenuAbsorbedSource>) -> Self {
        self.absorbed = absorbed;
        self
    }
}

/// One row a menu declares: what it draws, and what a tap on it does.
#[derive(Clone)]
struct LiquidMenuEntry {
    item: LiquidMenuItem,
    action: Option<Rc<dyn Fn()>>,
}

/// The scope a menu's rows are declared in.
///
/// Each row carries its own action, so a caller never dispatches on a row
/// index and never keeps a parallel list to look one up in. Rows appear in the
/// order they are declared.
pub struct LiquidMenuScope {
    entries: ScopeContent<LiquidMenuEntry>,
    section_next: Cell<bool>,
}

impl LiquidMenuScope {
    /// An interactive row. Tapping it runs `on_click`, then dismisses the menu
    /// unless the item [keeps open](LiquidMenuItem::keeps_open()).
    pub fn item(&self, item: LiquidMenuItem, on_click: impl Fn() + 'static) {
        self.push(LiquidMenuEntry {
            item,
            action: Some(Rc::new(on_click)),
        });
    }

    /// A gray, non-interactive section title.
    pub fn header(&self, label: impl Into<String>) {
        self.push(LiquidMenuEntry {
            item: LiquidMenuItem::header(label),
            action: None,
        });
    }

    /// Starts a new section: the next row declared is drawn under a hairline.
    pub fn separator(&self) {
        self.section_next.set(true);
    }

    fn push(&self, mut entry: LiquidMenuEntry) {
        if self.section_next.replace(false) {
            entry.item.section_start = true;
        }
        self.entries.push(entry);
    }
}

/// Runs `content` and returns the rows it declared.
fn collect_entries(content: impl FnOnce(&LiquidMenuScope)) -> Vec<LiquidMenuEntry> {
    ScopeContent::collect(
        |entries| LiquidMenuScope {
            entries,
            section_next: Cell::new(false),
        },
        content,
    )
}

impl LiquidMenuAbsorbedSource {
    pub fn new(
        rect: Rect,
        spec: crate::widgets::GlassButtonSpec,
        diameter: f32,
        icon_path: &'static str,
    ) -> Self {
        Self {
            rect,
            spec,
            diameter,
            icon_path,
        }
    }
}

/// Layout parameters for a [`LiquidMenu`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiquidMenuSpec {
    /// Settled card width in dp (the reference menus vary: the App Store
    /// List/Grid menu is 250, the sort/filter menu ~66% of screen width).
    pub width: f32,
}

impl Default for LiquidMenuSpec {
    fn default() -> Self {
        Self { width: MENU_WIDTH }
    }
}

impl LiquidMenuSpec {
    pub fn new(width: f32) -> Self {
        Self {
            width: if width.is_finite() {
                width.max(120.0)
            } else {
                MENU_WIDTH
            },
        }
    }
}

const MENU_WIDTH: f32 = 250.0;
/// Headroom around the glass node for the card's drop shadow: the shadow
/// renders inside the node surface, so without this pad its blur cuts to
/// a hard-edged block at the node bounds (user-circled at the collapse's
/// 2275ms).
const MENU_SHADOW_PAD: f32 = 48.0;
const MENU_RADIUS: f32 = 32.0;
const MENU_GROW_DELAY: f32 = 0.050;
const MENU_SOURCE_SEPARATE_END: f32 = 0.06;
const MENU_CARD_WIDTH_GROW_START: f32 = 0.16;
const MENU_CARD_HEIGHT_GROW_START: f32 = 0.12;
const MENU_OVERSHOOT_SCALE: f32 = 0.30;
// Spring clocks timed against the menu-open sheet: the reference droplet is
// still circular at 166 ms, a soft blur at 400 ms and crisp near 533-600 ms;
// 120/50 had ours fully crisp by ~300-400 ms (~1.4x fast — spring time goes
// as 1/sqrt(k), so both halve).
const MENU_GROW_STIFFNESS: f32 = 62.0;
const MENU_REVEAL_STIFFNESS: f32 = 26.0;
const MENU_WIDTH_EASE_POWER: f32 = 4.5;
const MENU_HEIGHT_EASE_POWER: f32 = 18.0;
const MENU_HEIGHT_OVERSHOOT: f32 = 0.15;
const MENU_HEIGHT_OVERSHOOT_END: f32 = 0.52;
const MENU_VERTICAL_REBOUND: f32 = 18.0;
const MENU_VERTICAL_REBOUND_END: f32 = 0.70;
const MENU_SOURCE_HEIGHT_RATIO: f32 = 0.86;
const MENU_SOURCE_TARGET_Y_PROGRESS: f32 = 0.0;
/// How far the card's top edge sits below the anchor's top: the settled menu
/// swallows the anchor button ENTIRELY (the reference "…" disappears under
/// the glass, reading as a smudge; only mid-flight does its bump ride the
/// droplet edge).
const ANCHOR_OVERLAP: f32 = 0.0;
const ROW_PADDING_X: f32 = 20.0;
const ROW_PADDING_Y: f32 = 9.25;
/// Horizontal inset of the expanded accordion header's chip from the panel
/// edges (menu-expand f_045).
const CHIP_INSET_X: f32 = 10.0;
const MENU_CONTENT_INSET_Y: f32 = 9.5;
/// Width reserved for the leading checkmark column when any item is checkable.
const CHECK_COLUMN: f32 = 24.0;
const ICON_SIZE: f32 = 24.0;
const ICON_GAP: f32 = 12.0;
const MENU_LONG_PRESS_MS: u64 = 500;
const MENU_LONG_PRESS_SLOP: f32 = 12.0;
const MENU_CONTENT_BLUR: f32 = 14.0;
const MENU_CONTENT_BLUR_POWER: f32 = 0.65;
const MENU_CONTENT_ALPHA_POWER: f32 = 0.45;
const MENU_TRIGGER_GLASS_CUTOFF: f32 = 0.05;
const MENU_TRIGGER_ABSORPTION_MS: u64 = 36;
const MENU_TRIGGER_RESTORE_DELAY_MS: u64 = 205;
const MENU_SOURCE_FOREGROUND_HIDE_MS: u64 = 200;
const MENU_SOURCE_FOREGROUND_RESTORE_DELAY_MS: u64 = 205;
#[derive(Clone, Copy, Debug, PartialEq)]
struct MenuGestureSnapshot {
    active: bool,
    claimed: bool,
    start: Point,
    position: Point,
    release: Option<(u64, Point)>,
}

impl Default for MenuGestureSnapshot {
    fn default() -> Self {
        Self {
            active: false,
            claimed: false,
            start: Point::new(0.0, 0.0),
            position: Point::new(0.0, 0.0),
            release: None,
        }
    }
}

struct LiquidMenuGestureInner {
    snapshot: MutableState<MenuGestureSnapshot>,
    next_release: Cell<u64>,
    item_rects: RefCell<Vec<Rc<Cell<Rect>>>>,
}

/// Shared ownership channel between a menu trigger and its popup. The trigger
/// keeps receiving the original pointer after the popup appears; the popup
/// reads that live position and consumes its eventual release.
#[derive(Clone)]
pub struct LiquidMenuGesture {
    inner: Rc<LiquidMenuGestureInner>,
}

impl PartialEq for LiquidMenuGesture {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl LiquidMenuGesture {
    /// Whether a pointer currently owns this gesture (pressed on the
    /// trigger or sliding through the menu).
    pub fn is_pressed(&self) -> bool {
        self.inner.snapshot.get().active
    }

    /// The live pointer position while the gesture is active (window
    /// coordinates) — the trigger surface's touch-glow anchor.
    pub fn press_point(&self) -> Option<Point> {
        let snapshot = self.inner.snapshot.get();
        snapshot.active.then_some(snapshot.position)
    }

    fn new() -> Self {
        Self {
            inner: Rc::new(LiquidMenuGestureInner {
                snapshot: mutableStateOf(MenuGestureSnapshot::default()),
                next_release: Cell::new(0),
                item_rects: RefCell::new(Vec::new()),
            }),
        }
    }

    fn id(&self) -> usize {
        Rc::as_ptr(&self.inner) as usize
    }

    fn begin(&self, point: Point) {
        self.inner.snapshot.set(MenuGestureSnapshot {
            active: true,
            start: point,
            position: point,
            ..MenuGestureSnapshot::default()
        });
    }

    fn move_to(&self, point: Point) {
        let mut snapshot = self.inner.snapshot.get();
        if snapshot.active {
            snapshot.position = point;
            self.inner.snapshot.set(snapshot);
        }
    }

    fn claim(&self) {
        let mut snapshot = self.inner.snapshot.get();
        if snapshot.active && !snapshot.claimed {
            snapshot.claimed = true;
            self.inner.snapshot.set(snapshot);
        }
    }

    fn release(&self, point: Point) {
        let mut snapshot = self.inner.snapshot.get();
        if !snapshot.active {
            return;
        }
        snapshot.position = point;
        snapshot.active = false;
        if snapshot.claimed {
            let sequence = self.inner.next_release.get().wrapping_add(1);
            self.inner.next_release.set(sequence);
            snapshot.release = Some((sequence, point));
        }
        self.inner.snapshot.set(snapshot);
    }

    fn cancel(&self) {
        let mut snapshot = self.inner.snapshot.get();
        snapshot.active = false;
        snapshot.claimed = false;
        snapshot.release = None;
        self.inner.snapshot.set(snapshot);
    }

    fn snapshot(&self) -> MenuGestureSnapshot {
        self.inner.snapshot.get()
    }

    fn item_rect(&self, index: usize) -> Rc<Cell<Rect>> {
        let mut rects = self.inner.item_rects.borrow_mut();
        while rects.len() <= index {
            rects.push(Rc::new(Cell::new(Rect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            })));
        }
        Rc::clone(&rects[index])
    }

    fn item_at(&self, point: Point, items: &[LiquidMenuItem]) -> Option<usize> {
        self.inner
            .item_rects
            .borrow()
            .iter()
            .enumerate()
            .take(items.len())
            .find_map(|(index, rect)| {
                (!items[index].header && rect.get().contains(point.x, point.y)).then_some(index)
            })
    }
}

/// Remembers one continuous menu gesture channel.
#[composable]
#[track_caller]
pub fn rememberLiquidMenuGesture() -> LiquidMenuGesture {
    remember(LiquidMenuGesture::new).with(Clone::clone)
}

/// A neighboring glass icon source that keeps its material mounted while an
/// open menu owns and deforms its foreground.
#[composable]
#[allow(non_snake_case)]
pub fn LiquidMenuAbsorbedIconButton(
    modifier: Modifier,
    spec: crate::widgets::GlassButtonSpec,
    diameter: f32,
    transferred: bool,
    on_click: impl Fn() + 'static,
    icon_path: &'static str,
) {
    let foreground = cranpose_animation::animate_float_as_state_with_initial(
        1.0,
        if transferred { 0.0 } else { 1.0 },
        cranpose_animation::AnimationType::Tween(if transferred {
            cranpose_animation::AnimationSpec::tween(
                MENU_SOURCE_FOREGROUND_HIDE_MS,
                cranpose_animation::Easing::LinearEasing,
            )
        } else {
            cranpose_animation::AnimationSpec::tween(5, cranpose_animation::Easing::EaseOut)
                .with_delay(MENU_SOURCE_FOREGROUND_RESTORE_DELAY_MS)
        }),
        "menu-source-foreground-ownership",
    );
    crate::widgets::button::GlassIconButtonWithForegroundAlpha(
        modifier,
        spec,
        diameter,
        foreground.get(),
        on_click,
        icon_path,
    );
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MenuGeometryPhase {
    path: f32,
    width: f32,
    height: f32,
}

fn menu_geometry_phase(expanded: bool, appear: f32) -> MenuGeometryPhase {
    if expanded && appear > 1.0 {
        let settle = 1.0 + (appear - 1.0) * MENU_OVERSHOOT_SCALE;
        return MenuGeometryPhase {
            path: settle,
            width: settle,
            height: settle,
        };
    }

    let appear = appear.clamp(0.0, 1.0);
    if !expanded {
        let normalized = ((appear - 0.015) / 0.985).clamp(0.0, 1.0);
        return MenuGeometryPhase {
            path: normalized,
            width: 1.0 - (1.0 - normalized).powf(2.5),
            height: 1.0 - (1.0 - normalized).powf(14.0),
        };
    }

    if appear < MENU_GROW_DELAY {
        let source_merge = smoothstep(0.10, 0.58, appear / MENU_GROW_DELAY);
        return MenuGeometryPhase {
            path: 0.0,
            width: source_merge,
            height: source_merge,
        };
    }

    let path = ((appear - MENU_GROW_DELAY) / (1.0 - MENU_GROW_DELAY)).clamp(0.0, 1.0);
    let width_growth =
        ((path - MENU_CARD_WIDTH_GROW_START) / (1.0 - MENU_CARD_WIDTH_GROW_START)).clamp(0.0, 1.0);
    let height_growth = ((path - MENU_CARD_HEIGHT_GROW_START)
        / (1.0 - MENU_CARD_HEIGHT_GROW_START))
        .clamp(0.0, 1.0);
    let overshoot_phase = ((path - 0.50) / 0.50).clamp(0.0, 1.0);
    let overshoot = 0.040 * (std::f32::consts::PI * overshoot_phase).sin().max(0.0);
    let height_overshoot_phase = (height_growth / MENU_HEIGHT_OVERSHOOT_END).clamp(0.0, 1.0);
    let height_overshoot = MENU_HEIGHT_OVERSHOOT
        * (std::f32::consts::PI * height_overshoot_phase)
            .sin()
            .max(0.0);
    MenuGeometryPhase {
        path,
        width: 1.0 - (1.0 - width_growth).powf(MENU_WIDTH_EASE_POWER) + overshoot,
        height: 1.0 - (1.0 - height_growth).powf(MENU_HEIGHT_EASE_POWER) + height_overshoot,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MenuShape {
    center_x: f32,
    center_y: f32,
    width: f32,
    height: f32,
    radius: f32,
}

impl MenuShape {
    fn capsule(center_x: f32, center_y: f32, width: f32, height: f32) -> Self {
        Self {
            center_x,
            center_y,
            width,
            height,
            radius: -1.0,
        }
    }

    fn from_window_rect(rect: Rect, node_origin: Point) -> Option<Self> {
        (rect.width > 0.0 && rect.height > 0.0).then(|| {
            Self::capsule(
                rect.x + rect.width * 0.5 - node_origin.x,
                rect.y + rect.height * 0.5 - node_origin.y,
                rect.width,
                rect.height,
            )
        })
    }

    fn as_glass_shape(self) -> (f32, f32, f32, f32, f32) {
        (
            self.center_x,
            self.center_y,
            self.width,
            self.height,
            self.radius,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MenuMorphGeometry {
    primary: MenuShape,
    source: MenuShape,
    target: MenuShape,
    path: f32,
}

fn menu_source_shape(anchor: MenuShape, absorbed: &[MenuShape], target: MenuShape) -> MenuShape {
    let mut left = anchor.center_x - anchor.width * 0.5;
    let mut right = anchor.center_x + anchor.width * 0.5;
    let mut top = anchor.center_y - anchor.height * 0.5;
    let mut bottom = anchor.center_y + anchor.height * 0.5;
    for shape in absorbed {
        left = left.min(shape.center_x - shape.width * 0.5);
        right = right.max(shape.center_x + shape.width * 0.5);
        top = top.min(shape.center_y - shape.height * 0.5);
        bottom = bottom.max(shape.center_y + shape.height * 0.5);
    }

    let width = right - left;
    let cluster_height = bottom - top;
    let height = cluster_height
        .max(width * MENU_SOURCE_HEIGHT_RATIO)
        .min(target.height);
    let cluster_center_y = (top + bottom) * 0.5;
    MenuShape::capsule(
        (left + right) * 0.5,
        cluster_center_y + (target.center_y - cluster_center_y) * MENU_SOURCE_TARGET_Y_PROGRESS,
        width,
        height,
    )
}

fn interpolate_menu_shape(
    start: MenuShape,
    target: MenuShape,
    width_progress: f32,
    height_progress: f32,
) -> MenuShape {
    let lerp = |a: f32, b: f32, progress: f32| a + (b - a) * progress;
    MenuShape::capsule(
        lerp(start.center_x, target.center_x, width_progress),
        lerp(start.center_y, target.center_y, height_progress),
        lerp(start.width, target.width, width_progress),
        lerp(start.height, target.height, height_progress),
    )
}

fn menu_vertical_rebound(path: f32) -> f32 {
    if !(0.0..MENU_VERTICAL_REBOUND_END).contains(&path) {
        return 0.0;
    }

    let normalized = path / MENU_VERTICAL_REBOUND_END;
    let onset = smoothstep(0.0, 0.012, path);
    MENU_VERTICAL_REBOUND
        * onset
        * (std::f32::consts::PI * normalized.powf(0.45))
            .sin()
            .max(0.0)
}

fn menu_morph_geometry(
    expanded: bool,
    appear: f32,
    anchor: MenuShape,
    absorbed: &[MenuShape],
    target: MenuShape,
) -> MenuMorphGeometry {
    let phase = menu_geometry_phase(expanded, appear);
    let source = menu_source_shape(anchor, absorbed, target);
    let mut primary = if expanded && phase.path < MENU_SOURCE_SEPARATE_END {
        anchor
    } else {
        let start = if expanded { source } else { anchor };
        interpolate_menu_shape(start, target, phase.width, phase.height)
    };
    if expanded && phase.path >= MENU_SOURCE_SEPARATE_END {
        // The droplet stays PINNED at its birth anchor while it swells
        // (reference 66-166ms: the blob grows on the filter button) and
        // only then descends to the panel position — a linear descent had
        // it drifting low from the first frames.
        let descent = smoothstep(0.10, 0.90, phase.path);
        primary.center_y = source.center_y + (target.center_y - source.center_y) * descent;
        primary.center_y += menu_vertical_rebound(phase.path);
    }
    let blob_radius = primary.height * 0.5;
    // The reference droplet keeps its capsule-fat corners through most of
    // the growth (menu-open f_043..f_055: corners ~40-50% of height, edges
    // bowed) and squares off only near settle — squaring at mid-path read
    // as "just a rounded rectangle" (user feedback items 2/6b).
    let squareness = smoothstep(0.55, 0.88, phase.path);
    primary.radius = if !expanded {
        blob_radius
    } else if phase.path >= 1.0 {
        target.radius
    } else {
        blob_radius + (target.radius - blob_radius) * squareness
    };
    MenuMorphGeometry {
        primary,
        source,
        target,
        path: phase.path,
    }
}

fn menu_ellipse_blend(path: f32) -> f32 {
    // The organic bow (SDF blended toward an ellipse) rides the WHOLE
    // growth and releases only as the panel squares for settle — dying at
    // 0.62 dropped the droplet into a rounded rectangle mid-flight.
    0.5 * smoothstep(0.06, 0.22, path) * (1.0 - smoothstep(0.62, 0.88, path))
}

fn menu_content_progress(expanded: bool, appear: f32, reveal: f32) -> f32 {
    if expanded {
        smoothstep(0.17, 0.82, reveal)
    } else {
        smoothstep(0.20, 0.75, appear)
    }
}

fn menu_content_blur(progress: f32) -> f32 {
    MENU_CONTENT_BLUR * (1.0 - progress.clamp(0.0, 1.0)).powf(MENU_CONTENT_BLUR_POWER)
}

fn menu_content_alpha(progress: f32) -> f32 {
    progress.clamp(0.0, 1.0).powf(MENU_CONTENT_ALPHA_POWER)
}

fn menu_content_scale(progress: f32) -> f32 {
    0.80 + 0.20 * progress.clamp(0.0, 1.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MenuAbsorbedVisualPhase {
    foreground_alpha: f32,
    backdrop_alpha: f32,
    foreground_blur: f32,
    scale_x: f32,
    scale_y: f32,
}

fn menu_absorbed_visual_phase(appear: f32, path: f32) -> MenuAbsorbedVisualPhase {
    let appear = appear.clamp(0.0, 1.0);
    let path = path.clamp(0.0, 1.0);
    let shrink = smoothstep(0.0, 0.24, path);
    let base_scale = 1.0 - 0.25 * shrink;
    let stretch = smoothstep(0.30, 0.56, path);
    // The neighbor stays fully readable past ~100ms of the open (menu-open
    // sheet: the reference "…" is crisp at 66-100ms), dims to a ghost as
    // the droplet thickens over it (~150-250ms on the grow spring), and
    // melts away by ~350ms.
    let handoff = smoothstep(0.30, 0.55, appear);
    let readable_alpha = 1.0 + (0.40 - 1.0) * handoff;
    // Once swallowed, the source is a chip-sized smudge the droplet's own
    // frost dissolves — the reference shows only a faint ghost of the blue
    // chip through the growing glass, never a hot stretched orb.
    MenuAbsorbedVisualPhase {
        foreground_alpha: readable_alpha * (1.0 - smoothstep(0.45, 0.85, path)),
        backdrop_alpha: 0.62 * smoothstep(0.45, 0.85, path),
        foreground_blur: 7.0 * smoothstep(0.45, 0.85, path),
        scale_x: base_scale * (1.0 + 0.20 * stretch),
        scale_y: base_scale * (1.0 + 0.28 * stretch),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MenuSurfacePhase {
    anchor_presence: f32,
    glue: f32,
    wobble: f32,
    bulge: f32,
}

fn menu_surface_phase(expanded: bool, appear: f32, path: f32) -> MenuSurfacePhase {
    let appear = appear.clamp(0.0, 1.0);
    let path = path.clamp(0.0, 1.0);
    let activity = (std::f32::consts::PI * path).sin().max(0.0);
    if expanded && path <= f32::EPSILON {
        let recoil = (appear / MENU_GROW_DELAY).clamp(0.0, 1.0);
        return MenuSurfacePhase {
            anchor_presence: 0.0,
            glue: 0.0,
            wobble: 0.18 * (std::f32::consts::PI * recoil).sin().max(0.0),
            bulge: 0.0,
        };
    }
    if expanded {
        return MenuSurfacePhase {
            // The primary is the anchor lobe. Adding a second full anchor at
            // the same coordinates masks the recoil and makes it look static.
            anchor_presence: 0.0,
            glue: 0.0,
            wobble: 0.08 * activity,
            bulge: 0.35 * activity,
        };
    }
    MenuSurfacePhase {
        anchor_presence: 0.0,
        glue: 0.0,
        wobble: 0.04 * activity,
        bulge: 0.25 * activity,
    }
}

fn menu_absorbed_shape_presence(path: f32) -> f32 {
    1.0 - smoothstep(MENU_SOURCE_SEPARATE_END, 0.30, path)
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Attaches the shared menu-trigger gesture to ANY surface (the circular
/// nav button, a filter pill, ...): a tap opens the menu; a long press
/// claims the gesture and opens it while the finger is still down, and the
/// SAME stream then slides through the opened menu's rows — keeps-open
/// accordion rows included — committing on release.
pub fn liquid_menu_trigger_input(
    modifier: Modifier,
    gesture: LiquidMenuGesture,
    on_open: impl Fn() + 'static,
) -> Modifier {
    let gate = remember(|| {
        let runtime = cranpose_core::with_current_composer(|composer| composer.runtime_handle());
        Rc::new(RefCell::new(cranpose_animation::Animatable::new(
            0.0, runtime,
        )))
    })
    .with(Rc::clone);
    let on_open: Rc<dyn Fn()> = Rc::new(on_open);

    let snapshot = gesture.snapshot();
    let gate_progress = gate.borrow().state().value();
    if gate_progress >= 1.0 && snapshot.active && !snapshot.claimed {
        gesture.claim();
        let on_open = Rc::clone(&on_open);
        SideEffect(move || on_open());
    }

    modifier.pointer_input(gesture.id(), {
        let gesture = gesture.clone();
        let gate = Rc::clone(&gate);
        let on_open = Rc::clone(&on_open);
        move |scope: PointerInputScope| {
            let gesture = gesture.clone();
            let gate = Rc::clone(&gate);
            let on_open = Rc::clone(&on_open);
            async move {
                scope
                    .await_pointer_event_scope(|await_scope| async move {
                        let mut active_pointer = Option::<PointerId>::None;
                        let mut moved = false;
                        loop {
                            let event = await_scope.await_pointer_event().await;
                            match event.kind {
                                PointerEventKind::Down if active_pointer.is_none() => {
                                    active_pointer = Some(event.id);
                                    moved = false;
                                    gesture.begin(event.global_position);
                                    let mut timer = gate.borrow_mut();
                                    timer.snapTo(0.0);
                                    timer.animateTo(
                                        1.0,
                                        cranpose_animation::AnimationType::Tween(
                                            cranpose_animation::AnimationSpec::linear(
                                                MENU_LONG_PRESS_MS,
                                            ),
                                        ),
                                    );
                                    event.consume();
                                }
                                PointerEventKind::Move if active_pointer == Some(event.id) => {
                                    gesture.move_to(event.global_position);
                                    let state = gesture.snapshot();
                                    let dx = event.global_position.x - state.start.x;
                                    let dy = event.global_position.y - state.start.y;
                                    if !state.claimed
                                        && dx * dx + dy * dy
                                            > MENU_LONG_PRESS_SLOP * MENU_LONG_PRESS_SLOP
                                    {
                                        moved = true;
                                        gate.borrow_mut().snapTo(0.0);
                                    }
                                    event.consume();
                                }
                                PointerEventKind::Up if active_pointer == Some(event.id) => {
                                    active_pointer = None;
                                    let claimed = gesture.snapshot().claimed;
                                    gate.borrow_mut().snapTo(0.0);
                                    if claimed {
                                        gesture.release(event.global_position);
                                    } else {
                                        gesture.cancel();
                                        if !moved {
                                            on_open();
                                        }
                                    }
                                    event.consume();
                                }
                                PointerEventKind::Cancel if active_pointer == Some(event.id) => {
                                    active_pointer = None;
                                    gate.borrow_mut().snapTo(0.0);
                                    gesture.cancel();
                                    event.consume();
                                }
                                _ => {}
                            }
                        }
                    })
                    .await;
            }
        }
    })
}

/// A glass icon trigger that owns one continuous menu gesture. A short click
/// opens normally; a hold opens while still pressed, then the same pointer can
/// slide over popup rows and release to fire one.
#[allow(clippy::too_many_arguments)]
#[composable]
#[allow(non_snake_case)]
pub fn LiquidMenuIconButton(
    modifier: Modifier,
    spec: crate::widgets::GlassButtonSpec,
    diameter: f32,
    covered: bool,
    gesture: LiquidMenuGesture,
    on_open: impl Fn() + 'static,
    icon_path: &'static str,
) {
    let interaction = rememberMutableInteractionSource();
    let (pressed_modifier, _, content_alpha) =
        crate::motion::liquid_press_scale(Modifier::empty(), interaction, 1.12);
    let trigger_visual = cranpose_animation::animate_float_as_state_with_initial(
        1.0,
        if covered { 0.0 } else { 1.0 },
        cranpose_animation::AnimationType::Tween(if covered {
            cranpose_animation::AnimationSpec::tween(
                MENU_TRIGGER_ABSORPTION_MS,
                cranpose_animation::Easing::EaseOut,
            )
        } else {
            cranpose_animation::AnimationSpec::tween(5, cranpose_animation::Easing::EaseOut)
                .with_delay(MENU_TRIGGER_RESTORE_DELAY_MS)
        }),
        "menu-trigger-absorption",
    );
    let gate = remember(|| {
        let runtime = cranpose_core::with_current_composer(|composer| composer.runtime_handle());
        Rc::new(RefCell::new(cranpose_animation::Animatable::new(
            0.0, runtime,
        )))
    })
    .with(Rc::clone);
    let on_open: Rc<dyn Fn()> = Rc::new(on_open);

    let snapshot = gesture.snapshot();
    let gate_progress = gate.borrow().state().value();
    if gate_progress >= 1.0 && snapshot.active && !snapshot.claimed {
        gesture.claim();
        let on_open = Rc::clone(&on_open);
        SideEffect(move || on_open());
    }

    let input = Modifier::empty()
        .size(Size::new(diameter, diameter))
        .pointer_input(gesture.id(), {
            let gesture = gesture.clone();
            let gate = Rc::clone(&gate);
            let on_open = Rc::clone(&on_open);
            move |scope: PointerInputScope| {
                let gesture = gesture.clone();
                let gate = Rc::clone(&gate);
                let on_open = Rc::clone(&on_open);
                async move {
                    scope
                        .await_pointer_event_scope(|await_scope| async move {
                            let mut active_pointer = Option::<PointerId>::None;
                            let mut moved = false;
                            let mut press: Option<PressInteractionPress> = None;
                            loop {
                                let event = await_scope.await_pointer_event().await;
                                match event.kind {
                                    PointerEventKind::Down if active_pointer.is_none() => {
                                        active_pointer = Some(event.id);
                                        moved = false;
                                        gesture.begin(event.global_position);
                                        press = Some(interaction.press(event.position));
                                        let mut timer = gate.borrow_mut();
                                        timer.snapTo(0.0);
                                        timer.animateTo(
                                            1.0,
                                            cranpose_animation::AnimationType::Tween(
                                                cranpose_animation::AnimationSpec::linear(
                                                    MENU_LONG_PRESS_MS,
                                                ),
                                            ),
                                        );
                                        event.consume();
                                    }
                                    PointerEventKind::Move if active_pointer == Some(event.id) => {
                                        gesture.move_to(event.global_position);
                                        let state = gesture.snapshot();
                                        let dx = event.global_position.x - state.start.x;
                                        let dy = event.global_position.y - state.start.y;
                                        if !state.claimed
                                            && dx * dx + dy * dy
                                                > MENU_LONG_PRESS_SLOP * MENU_LONG_PRESS_SLOP
                                        {
                                            moved = true;
                                            gate.borrow_mut().snapTo(0.0);
                                        }
                                        event.consume();
                                    }
                                    PointerEventKind::Up if active_pointer == Some(event.id) => {
                                        active_pointer = None;
                                        let claimed = gesture.snapshot().claimed;
                                        gate.borrow_mut().snapTo(0.0);
                                        if claimed {
                                            gesture.release(event.global_position);
                                        } else {
                                            gesture.cancel();
                                            if !moved {
                                                on_open();
                                            }
                                        }
                                        if let Some(active_press) = press.take() {
                                            interaction.release(active_press);
                                        }
                                        event.consume();
                                    }
                                    PointerEventKind::Cancel
                                        if active_pointer == Some(event.id) =>
                                    {
                                        active_pointer = None;
                                        gate.borrow_mut().snapTo(0.0);
                                        gesture.cancel();
                                        if let Some(active_press) = press.take() {
                                            interaction.cancel(active_press);
                                        }
                                        event.consume();
                                    }
                                    _ => {}
                                }
                            }
                        })
                        .await;
                }
            }
        });

    Box(
        pressed_modifier
            .then(modifier)
            .size(Size::new(diameter, diameter)),
        BoxSpec::default().content_alignment(cranpose_ui_layout::Alignment::CENTER),
        move || {
            let visual_alpha = trigger_visual.get().clamp(0.0, 1.0);
            let melt = 1.0 - visual_alpha;
            let visual_spec = spec.clone();
            let visual = Modifier::empty()
                .size(Size::new(diameter, diameter))
                .graphics_layer(move || GraphicsLayer {
                    alpha: visual_alpha * content_alpha.get().clamp(0.0, 1.0),
                    scale_x: 1.0 - 0.12 * melt,
                    scale_y: 1.0 - 0.12 * melt,
                    ..Default::default()
                });
            Box(
                visual,
                BoxSpec::default().content_alignment(cranpose_ui_layout::Alignment::CENTER),
                move || {
                    if visual_alpha > MENU_TRIGGER_GLASS_CUTOFF {
                        crate::widgets::GlassIconButton(
                            Modifier::empty(),
                            visual_spec.clone(),
                            diameter,
                            || {},
                            icon_path,
                        );
                    }
                },
            );
            Box(input.clone(), BoxSpec::default(), || {});
        },
    );
}

#[composable]
#[allow(non_snake_case)]
fn AbsorbedSourceVisual(
    source: LiquidMenuAbsorbedSource,
    node_origin: Point,
    alpha: f32,
    blur: f32,
    scale_x: f32,
    scale_y: f32,
) {
    if alpha <= 0.001 {
        return;
    }

    let diameter = source.diameter;
    let foreground_spec = source.spec.clone();
    let layer = Modifier::empty()
        .absolute_offset(source.rect.x - node_origin.x, source.rect.y - node_origin.y)
        .size(Size::new(diameter, diameter))
        .graphics_layer(move || GraphicsLayer {
            alpha,
            scale_x,
            scale_y,
            render_effect: (blur > 0.35).then(|| RenderEffect::blur(blur)),
            ..Default::default()
        });
    Box(
        layer,
        BoxSpec::default().content_alignment(cranpose_ui_layout::Alignment::CENTER),
        move || {
            crate::widgets::button::GlassIconForeground(
                foreground_spec.clone(),
                diameter,
                source.icon_path,
            );
        },
    );
}

/// A glass popup menu anchored to `anchor` (window coordinates of the button
/// that opened it). `absorbed` contains adjacent glass controls whose combined
/// source volume and foreground feed the opening droplet. While `expanded`,
/// taps outside dismiss via `on_dismiss`; a tap on a row runs that row's own
/// action and then dismisses, unless the row keeps the menu open.
///
/// ```rust,ignore
/// LiquidMenu(expanded, anchor, spec, Vec::new(), gesture, on_dismiss, |scope| {
///     scope.header("Show");
///     scope.item(LiquidMenuItem::new("List").checked(!grid), move || grid_state.set(false));
///     scope.item(LiquidMenuItem::new("Grid").checked(grid), move || grid_state.set(true));
/// });
/// ```
///
/// Layout follows the iOS menu: an optional leading checkmark column (present
/// on every row once any item is checkable, so labels align), then the icon
/// column, then the label. Sections split with full-width hairlines; headers
/// are gray non-interactive rows.
#[composable]
#[allow(non_snake_case)]
pub fn LiquidMenu(
    expanded: bool,
    anchor: Rect,
    spec: LiquidMenuSpec,
    absorbed: Vec<LiquidMenuAbsorbedSource>,
    gesture: LiquidMenuGesture,
    on_dismiss: impl Fn() + 'static,
    content: impl FnOnce(&LiquidMenuScope),
) {
    let entries = Rc::new(collect_entries(content));
    let items: Vec<LiquidMenuItem> = entries.iter().map(|entry| entry.item.clone()).collect();
    let menu_width = spec.width;
    // The menu outlives `expanded` by one collapse animation: dismissing
    // deflates the droplet back into the anchor (the reference close morph)
    // before the popup unmounts.
    let visible = remember(|| mutableStateOf(false)).with(|s| *s);
    if expanded && !visible.get() {
        visible.set(true);
    }
    if !expanded && !visible.get() {
        return;
    }
    let colors = liquid_colors();
    let typography = liquid_typography();
    let on_item: Rc<dyn Fn(usize)> = Rc::new(move |index| {
        if let Some(action) = entries.get(index).and_then(|entry| entry.action.as_ref()) {
            action();
        }
    });
    let on_dismiss: Rc<dyn Fn()> = Rc::new(on_dismiss);
    let gesture_snapshot = gesture.snapshot();
    let gesture_hover = (gesture_snapshot.active && gesture_snapshot.claimed)
        .then(|| gesture.item_at(gesture_snapshot.position, &items))
        .flatten();
    // A continuous gesture HOLDING on an accordion row expands it without
    // releasing (the reference single-gesture submenu): a dwell gate arms
    // while the claimed gesture rests on a keeps-open row and fires its
    // action once, keeping the stream alive.
    let dwell_gate = remember(|| {
        let runtime = cranpose_core::with_current_composer(|composer| composer.runtime_handle());
        Rc::new(RefCell::new(cranpose_animation::Animatable::new(
            0.0f32, runtime,
        )))
    })
    .with(Rc::clone);
    let dwell_row = remember(|| Rc::new(Cell::new(Option::<usize>::None))).with(Rc::clone);
    let dwell_fired = remember(|| Rc::new(Cell::new(Option::<usize>::None))).with(Rc::clone);
    {
        let hover_accordion =
            gesture_hover.filter(|index| items.get(*index).is_some_and(|item| item.keeps_open));
        if hover_accordion != dwell_row.get() {
            dwell_row.set(hover_accordion);
            dwell_fired.set(None);
            let mut gate = dwell_gate.borrow_mut();
            gate.snapTo(0.0);
            if hover_accordion.is_some() {
                gate.animateTo(
                    1.0,
                    cranpose_animation::AnimationType::Tween(
                        cranpose_animation::AnimationSpec::linear(450),
                    ),
                );
            }
        }
        let gate_value = dwell_gate.borrow().state().get();
        if gate_value >= 1.0 {
            if let Some(index) = dwell_row.get() {
                if dwell_fired.get() != Some(index) {
                    dwell_fired.set(Some(index));
                    let on_item_dwell = Rc::clone(&on_item);
                    SideEffect(move || on_item_dwell(index));
                }
            }
        }
    }
    let handled_release = remember(|| Rc::new(Cell::new(0u64))).with(Rc::clone);
    if let Some((sequence, point)) = gesture_snapshot.release {
        if handled_release.get() != sequence {
            handled_release.set(sequence);
            if let Some(index) = gesture.item_at(point, &items) {
                let keeps_open = items.get(index).is_some_and(|item| item.keeps_open);
                let on_item = Rc::clone(&on_item);
                let on_dismiss = Rc::clone(&on_dismiss);
                SideEffect(move || commit_menu_row(index, keeps_open, &on_item, &on_dismiss));
            }
        }
    }

    // The droplet spring: opening uses the bouncy morph spring (visible size
    // overshoot, the reference menu swells a few percent past its final width
    // and relaxes); closing is a faster, non-bouncy suck-back.
    // Open ≈400ms press→crisp with a soft overshoot (timed against the
    // reference recording); close is a faster suck-back (~200ms).
    let grow = cranpose_animation::animate_float_as_state_with_initial(
        0.0,
        if expanded { 1.0 } else { 0.0 },
        if expanded {
            cranpose_animation::spring(0.78, MENU_GROW_STIFFNESS)
        } else {
            cranpose_animation::AnimationType::Tween(cranpose_animation::AnimationSpec::linear(205))
        },
        "menu-grow",
    );
    // Content reveal has its own critically damped clock: rows begin as
    // smudges during growth and finish sharpening no later than shape settle.
    // Closing snaps the blur back on fast.
    let reveal_anim = cranpose_animation::animate_float_as_state_with_initial(
        0.0,
        if expanded { 1.0 } else { 0.0 },
        if expanded {
            cranpose_animation::spring(1.0, MENU_REVEAL_STIFFNESS)
        } else {
            cranpose_animation::spring(1.0, 900.0)
        },
        "menu-reveal",
    );
    // Body-level read: each animation frame recomposes this menu, which
    // re-registers fresh popup content (see `Popup`), driving the morph.
    // NOT clamped at 1 — the spring's overshoot is the size overshoot.
    let appear = grow.get().max(0.0);
    let reveal = reveal_anim.get().clamp(0.0, 1.0);
    if !expanded && appear < 0.02 {
        visible.set(false);
        return;
    }

    // The node spans from the anchor's top; ANCHOR_OVERLAP places the card's
    // top edge so the settled glass swallows the anchor button entirely.
    let anchor_zone = anchor.height * ANCHOR_OVERLAP;
    let node_size =
        remember(|| Rc::new(Cell::new(cranpose_ui_graphics::Size::ZERO))).with(Rc::clone);
    // Accordion: swapping `items` while the menu is open morphs the surface
    // to its new measured size in place (the reference expand grows the
    // container with overshoot while the incoming rows materialize). The
    // resize spring runs 0 -> 1 from the previous measured height.
    let resize_anim = remember(|| {
        let runtime = cranpose_core::with_current_composer(|composer| composer.runtime_handle());
        Rc::new(RefCell::new(cranpose_animation::Animatable::new(
            1.0f32, runtime,
        )))
    })
    .with(Rc::clone);
    let resize_from_h = remember(|| Rc::new(Cell::new(0.0f32))).with(Rc::clone);
    let items_signature: String = items
        .iter()
        .map(|item| {
            format!(
                "{}|{}|{}{}{}{}{};",
                item.label,
                item.subtitle.as_deref().unwrap_or(""),
                item.checked as u8,
                item.destructive as u8,
                item.section_start as u8,
                item.header as u8,
                item.keeps_open as u8,
            )
        })
        .collect();
    let last_signature = remember(|| Rc::new(RefCell::new(String::new()))).with(Rc::clone);
    if *last_signature.borrow() != items_signature {
        let was_open = !last_signature.borrow().is_empty()
            && expanded
            && grow.get() > 0.5
            && node_size.get().height > 1.0;
        *last_signature.borrow_mut() = items_signature;
        if was_open {
            resize_from_h.set(node_size.get().height);
            let mut anim = resize_anim.borrow_mut();
            anim.snapTo(0.0);
            anim.animateTo(1.0, cranpose_animation::spring(0.78, 170.0));
        }
    }
    let resize_state = resize_anim.borrow().state();
    // Right-align the card under the anchor (menus morph out of trailing
    // buttons), staying on-screen for anchors near the right edge. The host
    // renders the outside-tap scrim only while the menu is interactive. The
    // visual popup outlives `expanded` for its close morph, but its modal hit
    // surface must disappear immediately or a suspended screen can restore an
    // invisible scrim that absorbs the user's next action.
    let scrim_dismiss = Rc::clone(&on_dismiss);
    PopupDismissableWhen(
        expanded,
        anchor,
        Point::new(
            anchor.width - menu_width - MENU_SHADOW_PAD,
            -MENU_SHADOW_PAD,
        ),
        move || scrim_dismiss(),
        {
            let absorbed = absorbed.clone();
            let items = items.clone();
            let typography = typography.clone();
            let on_item = Rc::clone(&on_item);
            let on_dismiss = Rc::clone(&on_dismiss);
            let node_size = Rc::clone(&node_size);
            let gesture = gesture.clone();
            move || {
                // Shapeshift (the WWDC menu-open keyframes): the anchor
                // bubble inflates into the menu card as one droplet. The
                // anchor starts as its birth lobe and melts flat into the
                // settled edge.
                let anchor_center = (
                    menu_width - anchor.width * 0.5 + MENU_SHADOW_PAD,
                    anchor.height * 0.5 + MENU_SHADOW_PAD,
                );
                let anchor_shape = MenuShape::capsule(
                    anchor_center.0,
                    anchor_center.1,
                    anchor.width,
                    anchor.height,
                );
                let node_origin = Point::new(
                    anchor.x + anchor.width - menu_width - MENU_SHADOW_PAD,
                    anchor.y - MENU_SHADOW_PAD,
                );
                let absorbed_shapes: Vec<MenuShape> = absorbed
                    .iter()
                    .filter_map(|source| MenuShape::from_window_rect(source.rect, node_origin))
                    .collect();
                let morph_size = Rc::clone(&node_size);
                // Muted vibrancy: the absorbed button must read as a soft
                // smudge beneath the glass, not a hot saturated orb.
                // Scheme-aware body. The dark reference menu is NOT a heavy
                // dark tint: measured on menu-expand f_020, deep purple
                // (58,17,58) beneath reads (155,76,154) and the white page
                // reads (189) through the same panel — a strong tone
                // compression toward a bright pivot (out=(in-0.60)*0.37+0.60)
                // with vibrancy, so dark saturated backdrops BLOOM while
                // light ones dim. The old 205-alpha tint painted that band
                // instead of transmitting it.
                let glass = Glass::regular()
                    .shape(LiquidShape::RoundedRect(MENU_RADIUS))
                    // The frost foreground must be THIS menu's label color:
                    // glass_effect_with re-resolves the theme inside the
                    // popup content closure, which executes under the HOST
                    // theme — a dark menu on a light app inherited the
                    // light label (17,17,20) and the black-on-black
                    // protection lifted the panel over its dark purple
                    // header (+0.47 luma — the mauve band that defied every
                    // tone calibration; identity-material probe matched the
                    // frost arithmetic to 4 gray levels).
                    // Strength 0.18: the reference body holds luma ~0.51
                    // under white rows (menu-expand f_020 mid); 0.65 dimmed
                    // it to 0.32.
                    .adaptive_frost(colors.label, 0.18)
                    // iOS-scale frost: the reference panel smears the bright
                    // magenta pill beneath into a full-width bloom band
                    // (menu-expand f_020 top third) and washes mid-phase
                    // backdrop text into uniform haze (menu-open T166) — a
                    // 12dp blur kept both as localized ghosts.
                    .blur_radius(30.0)
                    .saturation(if colors.is_dark { 1.90 } else { 1.55 })
                    .lift(if colors.is_dark { 0.10 } else { 0.58 })
                    .highlight(0.14);
                let glass = if colors.is_dark {
                    // Two-point solve on menu-expand f_045: face over the
                    // dark header (128,83,132) and over the white page
                    // (90,81,91) — our slope matched but both endpoints sat
                    // ~+45 luma; the absorption tint carries the drop.
                    glass
                        .contrast(0.37)
                        .tint(Color::from_rgba_u8(34, 10, 34, 146))
                } else {
                    glass
                };
                let glass = glass
                    .shadow_style(GlassShadow::new(
                        // Measured on menu-expand f_045: the page 15dp from
                        // the panel edge falls 254 -> ~85 (alpha ~0.65) and
                        // recovers fully ~150dp out — the dark presentation
                        // carries a strong, wide ambient, not a card hint.
                        Color::BLACK.with_alpha(if colors.is_dark { 0.60 } else { 0.11 }),
                        if colors.is_dark { 32.0 } else { 26.0 },
                        if colors.is_dark { 10.0 } else { 8.0 },
                        0.0,
                    ))
                    .no_clip();
                let resize_from = Rc::clone(&resize_from_h);
                // The hovered/drag-through row lights the SURFACE via the
                // touch glow (saturation + soft light under the finger),
                // not just a flat recolor: composition publishes the active
                // row's center; the glass reads it per frame.
                let glow_point: Rc<Cell<Option<(f32, f32)>>> =
                    remember(|| Rc::new(Cell::new(None))).with(Rc::clone);
                let glow_for_glass = Rc::clone(&glow_point);
                let glass_node_origin = node_origin;
                // The birth droplet reads as a FROSTED blob, not clear
                // glass: the dark menu births bright gray-white milk
                // (menu-expand 400-466ms), the light menu a cool frosted
                // near-white so the droplet has presence over the white
                // list content (menu-open f_043..f_055 is a distinct
                // frosted blob, not transparent).
                let birth_milk = Some(if colors.is_dark {
                    Color::from_rgba_u8(208, 204, 214, 240)
                } else {
                    Color::from_rgba_u8(246, 247, 250, 210)
                });
                let card = Modifier::empty()
                    .report_size(Rc::clone(&node_size))
                    .glass_effect_with(glass, move || {
                        let glow_touch = glow_for_glass.get().map(|(x, y)| {
                            (x - glass_node_origin.x, y - glass_node_origin.y, 1.0f32)
                        });
                        let size = morph_size.get();
                        let measured_h =
                            (size.height - anchor_zone - MENU_SHADOW_PAD * 2.0).max(24.0);
                        // Accordion resize: spring from the previous measured
                        // height toward the new one; damping 0.78 gives the
                        // reference's visible size overshoot.
                        let resize_t = resize_state.get();
                        let from_h =
                            (resize_from.get() - anchor_zone - MENU_SHADOW_PAD * 2.0).max(24.0);
                        let menu_h = if resize_from.get() > 1.0 {
                            from_h + (measured_h - from_h) * resize_t.max(0.0)
                        } else {
                            measured_h
                        };
                        // Pillowy settled corners (the reference menu's radius
                        // is ~0.26 of its height — far rounder than a desktop
                        // popup).
                        let settle_radius = (menu_h * 0.32).clamp(26.0, MENU_RADIUS);
                        let target = MenuShape {
                            center_x: menu_width * 0.5 + MENU_SHADOW_PAD,
                            center_y: anchor_zone + MENU_SHADOW_PAD + menu_h * 0.5,
                            width: menu_width,
                            height: menu_h,
                            radius: settle_radius,
                        };
                        let geometry = menu_morph_geometry(
                            expanded,
                            appear,
                            anchor_shape,
                            &absorbed_shapes,
                            target,
                        );
                        let t = geometry.path;
                        let primary = geometry.primary.as_glass_shape();
                        let start = if expanded {
                            geometry.source
                        } else {
                            anchor_shape
                        };
                        let target = geometry.target;
                        let surface = menu_surface_phase(expanded, appear, t);
                        // Growth direction from the anchor toward the card
                        // center (node coords, y down).
                        let dir_x = target.center_x - start.center_x;
                        let dir_y = target.center_y - start.center_y;
                        let mut bulge_dir = dir_y.atan2(dir_x);
                        if !expanded {
                            bulge_dir += std::f32::consts::PI;
                        }
                        let mut shapes = Vec::new();
                        if surface.anchor_presence > 0.01 {
                            shapes.push((
                                anchor_shape.center_x,
                                anchor_shape.center_y,
                                anchor_shape.width * surface.anchor_presence,
                                anchor_shape.height * surface.anchor_presence,
                                -1.0,
                            ));
                        }
                        let absorbed_presence = menu_absorbed_shape_presence(t);
                        if expanded && absorbed_presence > 0.01 {
                            shapes.extend(absorbed_shapes.iter().map(|shape| {
                                (
                                    shape.center_x,
                                    shape.center_y,
                                    shape.width * absorbed_presence,
                                    shape.height * absorbed_presence,
                                    -1.0,
                                )
                            }));
                        }
                        let glue = surface.glue;
                        let activity = if expanded {
                            // MILKY BIRTH: the droplet births as bright
                            // frosted milk and the panel material matures in
                            // as it grows (menu-expand target 400-533ms: a
                            // gray-white blob first, the purple panel after;
                            // menu-open births translucent). The old ramp
                            // reached full material within ~30ms of the
                            // touch, so the dark menu was born already dark.
                            smoothstep(0.0, 0.42, t)
                        } else {
                            // The closing panel keeps its full dark material
                            // until the geometry actually collapses (height
                            // eases with pow 14, so the size only moves in
                            // the last ~15% of the path). Draining from 0.65
                            // left a full-size pale ghost through the close.
                            smoothstep(0.0, 0.12, t)
                        };
                        GlassDynamics {
                            activity: Some(activity),
                            resting_tint: birth_milk,
                            touch: glow_touch,
                            morph: Some(GlassMorph {
                                node_size: (size.width.max(1.0), size.height.max(1.0)),
                                primary,
                                shapes,
                                glue,
                                wobble_amplitude: surface.wobble,
                                wobble_phase: t * 8.0,
                                bulge_amplitude: surface.bulge,
                                bulge_direction: bulge_dir,
                                ellipse_blend: menu_ellipse_blend(t),
                                deformation: None,
                                zoom_anchor: (0.0, 0.0),
                            }),
                            ..Default::default()
                        }
                    })
                    .width(menu_width + MENU_SHADOW_PAD * 2.0);

                let has_checks = items.iter().any(|item| item.checked);
                // Finger/pointer sliding through the menu highlights the row
                // under it (release selects) — the iOS drag-through-menu.
                let hovered = remember(|| mutableStateOf(Option::<usize>::None)).with(|s| *s);
                let glow_row = gesture_hover.or(hovered.get());
                glow_point.set(glow_row.map(|index| {
                    let rect = gesture.item_rect(index).get();
                    (rect.x + rect.width * 0.5, rect.y + rect.height * 0.5)
                }));
                let gesture = gesture.clone();
                let source_phase =
                    menu_absorbed_visual_phase(appear, menu_geometry_phase(expanded, appear).path);
                for source in absorbed.iter().cloned() {
                    AbsorbedSourceVisual(
                        source,
                        node_origin,
                        source_phase.backdrop_alpha,
                        0.0,
                        source_phase.scale_x,
                        source_phase.scale_y,
                    );
                }
                Box(card, BoxSpec::default(), {
                    let items = items.clone();
                    let typography = typography.clone();
                    let on_item = Rc::clone(&on_item);
                    let on_dismiss = Rc::clone(&on_dismiss);
                    move || {
                        Column(
                            Modifier::empty().fill_max_width().padding(MENU_SHADOW_PAD),
                            ColumnSpec::default(),
                            {
                                let items = items.clone();
                                let typography = typography.clone();
                                let on_item = Rc::clone(&on_item);
                                let on_dismiss = Rc::clone(&on_dismiss);
                                let gesture = gesture.clone();
                                move || {
                                    Box(
                                        Modifier::empty().height(anchor_zone),
                                        BoxSpec::default(),
                                        || {},
                                    );
                                    // The card's drop shadow belongs to the menu rect
                                    // only (the glass node also spans the anchor zone).
                                    // Soft and wide: the reference menu shadow is a
                                    // whisper. Content is absent on the initial stretch,
                                    // appears as a smudge during growth, and sharpens by
                                    // settle. While closing it rides the fast glass clock
                                    // so it contracts with the panel.
                                    let content = menu_content_progress(expanded, appear, reveal);
                                    // During an accordion resize the rows dip back
                                    // into the smudge and re-materialize as the
                                    // growth settles (reference expand frames).
                                    let resize_t = resize_state.get().clamp(0.0, 1.0);
                                    let content =
                                        content * (0.45 + 0.55 * smoothstep(0.35, 1.0, resize_t));
                                    // Rows materialize from behind the glass and scale with
                                    // the droplet from the anchor corner; the content lives
                                    // on the growing surface instead of fading at full size.
                                    let content_scale = menu_content_scale(content);
                                    let content_blur = menu_content_blur(content);
                                    let content_translation_y = if expanded {
                                        menu_vertical_rebound(
                                            menu_geometry_phase(expanded, appear).path,
                                        )
                                    } else {
                                        0.0
                                    };
                                    let rows_wrap = Modifier::empty()
                                        .fill_max_width()
                                        .graphics_layer(move || GraphicsLayer {
                                            alpha: menu_content_alpha(content),
                                            scale_x: content_scale,
                                            scale_y: content_scale,
                                            transform_origin:
                                                cranpose_ui_graphics::TransformOrigin {
                                                    pivot_fraction_x: 1.0,
                                                    pivot_fraction_y: 0.0,
                                                },
                                            translation_y: content_translation_y,
                                            render_effect: (content_blur > 0.35)
                                                .then(|| RenderEffect::blur(content_blur)),
                                            ..Default::default()
                                        });
                                    Box(rows_wrap, BoxSpec::default(), {
                                        let items = items.clone();
                                        let typography = typography.clone();
                                        let on_item = Rc::clone(&on_item);
                                        let on_dismiss = Rc::clone(&on_dismiss);
                                        let gesture = gesture.clone();
                                        move || {
                                            Column(
                                                Modifier::empty().fill_max_width().padding_each(
                                                    0.0,
                                                    MENU_CONTENT_INSET_Y,
                                                    0.0,
                                                    MENU_CONTENT_INSET_Y,
                                                ),
                                                ColumnSpec::default(),
                                                {
                                                    let items = items.clone();
                                                    let typography = typography.clone();
                                                    let on_item = Rc::clone(&on_item);
                                                    let on_dismiss = Rc::clone(&on_dismiss);
                                                    let gesture = gesture.clone();
                                                    move || {
                                                        for (index, item) in
                                                            items.iter().enumerate()
                                                        {
                                                            if item.section_start && index > 0 {
                                                                // Whisper-subtle: the
                                                                // reference surface reads
                                                                // nearly seamless.
                                                                let separator =
                                                                    colors.separator.with_alpha(
                                                                        colors.separator.a() * 0.22,
                                                                    );
                                                                Box(
                                                        Modifier::empty()
                                                            .fill_max_width()
                                                            .padding_symmetric(ROW_PADDING_X, 0.0)
                                                            .height(1.0)
                                                            .draw_behind(move |scope| {
                                                                scope.draw_rect(
                                                                    cranpose_ui_graphics::Brush::solid(
                                                                        separator,
                                                                    ),
                                                                );
                                                            }),
                                                        BoxSpec::default(),
                                                        || {},
                                                    );
                                                            }

                                                            if item.header {
                                                                menu_header_row(
                                                                    item,
                                                                    &typography,
                                                                    has_checks,
                                                                    colors,
                                                                );
                                                                continue;
                                                            }
                                                            // An accordion header with its children
                                                            // present is EXPANDED: the reference lifts
                                                            // it onto an inset rounded chip
                                                            // (menu-expand f_045: chip 145,120,146
                                                            // over body 90,81,91 — white ~0.33).
                                                            let expanded_header = item.keeps_open
                                                                && items
                                                                    .get(index + 1)
                                                                    .is_some_and(|next| {
                                                                        !next.keeps_open
                                                                            && !next.header
                                                                    });
                                                            menu_item_row(
                                                                index,
                                                                item,
                                                                &typography,
                                                                has_checks,
                                                                colors,
                                                                hovered,
                                                                gesture_hover,
                                                                expanded_header,
                                                                gesture.item_rect(index),
                                                                Rc::clone(&on_item),
                                                                Rc::clone(&on_dismiss),
                                                            );
                                                        }
                                                    }
                                                },
                                            );
                                        }
                                    });
                                }
                            },
                        );
                    }
                });
                for source in absorbed.iter().cloned() {
                    AbsorbedSourceVisual(
                        source,
                        node_origin,
                        source_phase.foreground_alpha,
                        source_phase.foreground_blur,
                        source_phase.scale_x,
                        source_phase.scale_y,
                    );
                }
            }
        },
    );
}

/// Renders an anchor and positions a [`LiquidMenu`] from the anchor's measured
/// window rectangle, without application-owned coordinate calculations.
#[composable]
#[allow(non_snake_case)]
pub fn LiquidDropdownMenu<A>(
    modifier: Modifier,
    expanded: bool,
    spec: LiquidDropdownMenuSpec,
    on_dismiss: impl Fn() + 'static,
    mut anchor_content: A,
    content: impl Fn(&LiquidMenuScope) + 'static,
) where
    A: FnMut() + 'static,
{
    let anchor = rememberMutableStateOf(|| Rect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    });
    let gesture = rememberLiquidMenuGesture();
    let on_dismiss = Rc::new(on_dismiss);
    let content = Rc::new(content);
    Box(
        modifier.report_window_rect_state(anchor),
        BoxSpec::default(),
        move || {
            anchor_content();
            let content = Rc::clone(&content);
            let dismiss = Rc::clone(&on_dismiss);
            LiquidMenu(
                expanded,
                anchor.get(),
                spec.menu,
                spec.absorbed.clone(),
                gesture.clone(),
                move || dismiss(),
                move |scope| content(scope),
            );
        },
    );
}

/// What a tap on a row does, wherever the tap came from.
///
/// A row can be committed two ways: its own pointer handler sees the release,
/// or a press that started on the trigger slides through the menu and lets go
/// over it. Both mean the same thing — run the row's action, and dismiss
/// unless the row asked to stay open — so both go through here. Written twice,
/// the two paths can disagree about whether a `keeps_open` row closes the menu,
/// which is precisely how a menu ends up dismissing a row that asked to stay.
fn commit_menu_row(
    index: usize,
    keeps_open: bool,
    on_item: &Rc<dyn Fn(usize)>,
    on_dismiss: &Rc<dyn Fn()>,
) {
    on_item(index);
    if !keeps_open {
        on_dismiss();
    }
}

/// Gray non-interactive section header, aligned with the icon column.
fn menu_header_row(
    item: &LiquidMenuItem,
    typography: &crate::theme::LiquidTypography,
    has_checks: bool,
    colors: crate::theme::LiquidColors,
) {
    let label = item.label.clone();
    let style = TextStyle {
        span_style: SpanStyle {
            color: Some(colors.secondary_label),
            font_size: TextUnit::Sp(13.0),
            ..typography.footnote.span_style.clone()
        },
        ..typography.footnote.clone()
    };
    let indent = ROW_PADDING_X + if has_checks { CHECK_COLUMN } else { 0.0 };
    let row = Modifier::empty()
        .fill_max_width()
        .padding_each(indent, 12.0, ROW_PADDING_X, 2.0);
    Row(row, RowSpec::default(), move || {
        Text(label.clone(), Modifier::empty(), style.clone());
    });
}

/// One interactive menu row: [check][icon][label].
#[allow(non_snake_case)]
#[allow(clippy::too_many_arguments)]
fn menu_item_row(
    index: usize,
    item: &LiquidMenuItem,
    typography: &crate::theme::LiquidTypography,
    has_checks: bool,
    colors: crate::theme::LiquidColors,
    hovered: cranpose_core::MutableState<Option<usize>>,
    gesture_hover: Option<usize>,
    expanded_header: bool,
    rect_sink: Rc<Cell<Rect>>,
    on_item: Rc<dyn Fn(usize)>,
    on_dismiss: Rc<dyn Fn()>,
) {
    let color = if item.destructive {
        colors.destructive
    } else {
        colors.label
    };
    let is_hovered = hovered.get() == Some(index) || gesture_hover == Some(index);
    let highlight = if colors.is_dark {
        cranpose_ui_graphics::Color::from_rgba_u8(120, 120, 128, 44)
    } else {
        cranpose_ui_graphics::Color::from_rgba_u8(120, 120, 128, 30)
    };
    let row_label = item.label.clone();
    let keeps_open = item.keeps_open;
    let row = Modifier::empty()
        .fill_max_width()
        .report_window_rect(rect_sink)
        .semantics(move |config| {
            config.role = Some(SemanticsWidgetRole::Button);
            config.is_clickable = true;
            config.content_description = Some(row_label.clone());
        })
        .pointer_input(index, {
            let on_item = Rc::clone(&on_item);
            let on_dismiss = Rc::clone(&on_dismiss);
            move |scope: PointerInputScope| {
                let on_item = Rc::clone(&on_item);
                let on_dismiss = Rc::clone(&on_dismiss);
                async move {
                    scope
                        .await_pointer_event_scope(|await_scope| async move {
                            loop {
                                let event = await_scope.await_pointer_event().await;
                                match event.kind {
                                    PointerEventKind::Enter | PointerEventKind::Move => {
                                        hovered.set(Some(index));
                                    }
                                    PointerEventKind::Exit if hovered.get() == Some(index) => {
                                        hovered.set(None);
                                    }
                                    PointerEventKind::Down => {
                                        hovered.set(Some(index));
                                        event.consume();
                                    }
                                    PointerEventKind::Up => {
                                        hovered.set(None);
                                        commit_menu_row(index, keeps_open, &on_item, &on_dismiss);
                                        event.consume();
                                    }
                                    _ => {}
                                }
                            }
                        })
                        .await;
                }
            }
        })
        .draw_behind(move |scope| {
            if expanded_header {
                // The expanded accordion header rides an inset chip: white
                // lift over the panel body (and over the header bleed at the
                // panel top it lands on the reference's muted lavender
                // instead of raw gradient heat).
                let chip = if colors.is_dark {
                    // Brighter chip: the reference active header is a light
                    // lavender band (menu-expand expand f_040 = 160,121,160);
                    // 0.30 left it a dark 85,55,89. The active/expanded
                    // header is a touched-up surface — it reads HDR-brighter.
                    // Light MAGENTA-white (not neutral): a pure-white chip
                    // read lavender (G too high); the reference band holds
                    // its magenta (160,121,160).
                    Color::from_rgb_u8(255, 224, 248).with_alpha(0.52)
                } else {
                    Color::BLACK.with_alpha(0.08)
                };
                let size = scope.size();
                scope.draw_round_rect_at(
                    Rect {
                        x: CHIP_INSET_X,
                        y: 0.0,
                        width: (size.width - CHIP_INSET_X * 2.0).max(0.0),
                        height: size.height,
                    },
                    Brush::solid(chip),
                    CornerRadii::uniform(16.0),
                );
            }
            if is_hovered {
                scope.draw_round_rect(Brush::solid(highlight), CornerRadii::uniform(14.0));
            }
        })
        .padding_symmetric(ROW_PADDING_X, ROW_PADDING_Y);

    let label = item.label.clone();
    let subtitle = item.subtitle.clone();
    let icon = item.icon;
    let checked = item.checked;
    let accordion_chevron = item.keeps_open && subtitle.is_some();
    let secondary = colors.secondary_label;
    let typography = typography.clone();
    Row(
        row,
        RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            let label = label.clone();
            let subtitle = subtitle.clone();
            if has_checks {
                // Leading checkmark column, reserved on every row so icons
                // and labels align (the reference "Show" menu).
                Box(
                    Modifier::empty().width(CHECK_COLUMN),
                    BoxSpec::default(),
                    move || {
                        if checked {
                            crate::icons::Icon(crate::icons::CHECK, 16.0, color);
                        }
                    },
                );
            }
            if let Some(icon) = icon {
                crate::icons::Icon(icon, ICON_SIZE, color);
                Box(Modifier::empty().width(ICON_GAP), BoxSpec::default(), || {});
            }
            let style = TextStyle {
                span_style: SpanStyle {
                    color: Some(color),
                    font_weight: Some(FontWeight::NORMAL),
                    ..typography.body.span_style.clone()
                },
                ..typography.body.clone()
            };
            if let Some(subtitle) = subtitle {
                // Two-line row: the reference sort/filter headers describe
                // their current state in a gray second line.
                let subtitle_style = TextStyle {
                    span_style: SpanStyle {
                        color: Some(secondary),
                        font_size: TextUnit::Sp(13.0),
                        ..typography.footnote.span_style.clone()
                    },
                    ..typography.footnote.clone()
                };
                Column(
                    Modifier::empty().weight(1.0),
                    ColumnSpec::default(),
                    move || {
                        Text(label.clone(), Modifier::empty(), style.clone());
                        Text(subtitle.clone(), Modifier::empty(), subtitle_style.clone());
                    },
                );
            } else {
                Text(label, Modifier::empty().weight(1.0), style);
            }
            if accordion_chevron {
                crate::icons::Icon(crate::icons::CHEVRON_DOWN, 18.0, secondary);
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scope_keeps_actions_with_their_items() {
        let selected = Rc::new(Cell::new(false));
        let selected_by_action = Rc::clone(&selected);
        let entries = collect_entries(|scope| {
            scope.header("Section");
            scope.separator();
            scope.item(LiquidMenuItem::new("Open"), move || {
                selected_by_action.set(true)
            });
        });

        assert_eq!(entries.len(), 2);
        assert!(entries[0].item.header);
        assert!(entries[1].item.section_start);
        entries[1].action.as_ref().unwrap()();
        assert!(selected.get());
    }

    #[test]
    fn dropdown_spec_builders_preserve_menu_configuration() {
        let menu = LiquidMenuSpec { width: 180.0 };
        let absorbed = LiquidMenuAbsorbedSource::new(
            Rect::from_origin_size(Point::ZERO, Size::new(44.0, 44.0)),
            crate::widgets::GlassButtonSpec::default(),
            44.0,
            crate::icons::SEARCH,
        );
        let spec = LiquidDropdownMenuSpec::default()
            .menu(menu)
            .absorbed(vec![absorbed]);
        assert_eq!(spec.menu, menu);
        assert_eq!(spec.absorbed.len(), 1);
    }

    #[test]
    fn menu_rows_use_the_reference_leading_grid_and_vertical_rhythm() {
        assert_eq!(ROW_PADDING_X, 20.0);
        assert_eq!(CHECK_COLUMN, 24.0);
        assert_eq!(ICON_SIZE, 24.0);
        assert_eq!(ICON_GAP, 12.0);

        let check_center = ROW_PADDING_X + 8.0;
        let icon_center = ROW_PADDING_X + CHECK_COLUMN + ICON_SIZE * 0.5;
        let label_start = ROW_PADDING_X + CHECK_COLUMN + ICON_SIZE + ICON_GAP;
        assert_eq!((check_center, icon_center, label_start), (28.0, 56.0, 80.0));

        let row_height = ICON_SIZE + ROW_PADDING_Y * 2.0;
        assert!((42.0..=43.0).contains(&row_height));
        assert_eq!(MENU_CONTENT_INSET_Y, 9.5);
        let two_row_panel_height = row_height * 2.0 + MENU_CONTENT_INSET_Y * 2.0;
        assert!((103.5..=104.5).contains(&two_row_panel_height));
    }

    #[test]
    fn menu_geometry_keeps_the_source_cluster_horizontal_before_card_growth() {
        let anchor = MenuShape::capsule(228.0, 22.0, 44.0, 44.0);
        let absorbed = [MenuShape::capsule(176.0, 22.0, 44.0, 44.0)];
        let target = MenuShape {
            center_x: 125.0,
            center_y: 52.0,
            width: 250.0,
            height: 104.0,
            radius: 32.0,
        };
        let pose = |appear| menu_morph_geometry(true, appear, anchor, &absorbed, target).primary;

        let initial = pose(0.0);
        assert_eq!((initial.width, initial.height), (44.0, 44.0));

        let merged = pose(0.028_576);
        assert_eq!(merged, initial);
        let source = menu_source_shape(anchor, &absorbed, target);
        assert_eq!(source.width, 96.0);
        assert!((82.5..=82.6).contains(&source.height));
        assert_eq!(source.center_y, anchor.center_y);
        assert_eq!(menu_absorbed_shape_presence(0.0), 1.0);
        assert_eq!(menu_absorbed_shape_presence(0.30), 0.0);

        let early = pose(0.070_208);
        let middle = pose(0.199_019);
        let broad = pose(0.539_174);
        assert_eq!(early, initial);
        assert_eq!(middle.width, source.width);
        assert!(middle.height >= source.height);
        assert!(broad.width > middle.width && broad.height <= target.height * 1.1);
        assert!(middle.width > middle.height);
        assert!(broad.width > broad.height * 2.0);

        let swell = pose(0.701_903);
        assert!(
            (252.0..=259.0).contains(&swell.width) && (102.0..=106.0).contains(&swell.height),
            "the broad body must overshoot horizontally without inflating vertically: {swell:?}"
        );
        assert!(menu_ellipse_blend(0.25) > 0.3);
        assert_eq!(menu_ellipse_blend(0.0), 0.0);
        assert_eq!(menu_ellipse_blend(1.0), 0.0);

        let overshoot = pose(1.08);
        assert!((250.0..=254.0).contains(&overshoot.width));
        assert!((104.0..=106.0).contains(&overshoot.height));
        assert_eq!(MENU_RADIUS, 32.0);
        assert!((0.045..=0.055).contains(&MENU_GROW_DELAY));
        // Sheet-timed: the reference droplet is still circular at 166 ms and
        // crisp near 533-600 ms — stiffness 120 had the panel formed by
        // ~300 ms.
        assert!((55.0..=70.0).contains(&MENU_GROW_STIFFNESS));
    }

    #[test]
    fn menu_open_spring_departs_early_then_settles_without_a_dead_interval() {
        let (source_phase, _) =
            cranpose_animation::advance_spring(0.0, 0.0, 1.0, 0.78, MENU_GROW_STIFFNESS, 0.054);
        assert!(
            source_phase > MENU_GROW_DELAY,
            "the departing oval must be visible by the target's early frame: {source_phase}"
        );
        let (broad_phase, _) =
            cranpose_animation::advance_spring(0.0, 0.0, 1.0, 0.78, MENU_GROW_STIFFNESS, 0.180);
        assert!(
            (0.35..=0.60).contains(&broad_phase),
            "the broad menu body must be established by 180ms: {broad_phase}"
        );
        let (settled_phase, _) =
            cranpose_animation::advance_spring(0.0, 0.0, 1.0, 0.78, MENU_GROW_STIFFNESS, 0.600);
        assert!(settled_phase > 0.95);
    }

    #[test]
    fn menu_body_uses_the_shared_vertical_rebound_path() {
        let anchor = MenuShape::capsule(228.0, 22.0, 44.0, 44.0);
        let absorbed = [MenuShape::capsule(176.0, 22.0, 44.0, 44.0)];
        let target = MenuShape {
            center_x: 125.0,
            center_y: 52.0,
            width: 250.0,
            height: 104.0,
            radius: 32.0,
        };

        let source = menu_source_shape(anchor, &absorbed, target);
        for appear in [0.199_019, 0.296_780, 0.412_956, 0.539_174] {
            let geometry = menu_morph_geometry(true, appear, anchor, &absorbed, target);
            let phase = menu_geometry_phase(true, appear);
            // The body descends on the eased path (pinned at the birth
            // anchor while swelling), plus the shared rebound.
            let descent = smoothstep(0.10, 0.90, phase.path);
            let interpolated_y = source.center_y + (target.center_y - source.center_y) * descent;
            let expected_y = interpolated_y + menu_vertical_rebound(phase.path);
            assert!(
                (geometry.primary.center_y - expected_y).abs() < 0.001,
                "body and content must resolve the same rebound path: {geometry:?}"
            );
        }
        assert_eq!(menu_vertical_rebound(0.0), 0.0);
        assert!(menu_vertical_rebound(0.25) > 0.0);
        assert_eq!(menu_vertical_rebound(MENU_VERTICAL_REBOUND_END), 0.0);
    }

    #[test]
    fn menu_close_reverses_through_a_smooth_oval() {
        let phase = menu_geometry_phase(false, 0.6);
        assert!(
            phase.width > 0.80,
            "the close must retain its broad body at mid-flight: {phase:?}"
        );
        assert!(
            44.0 + (250.0 - 44.0) * phase.width > 1.8 * (44.0 + (104.0 - 44.0) * phase.height),
            "the close must pass back through the wide oval in physical dimensions: {phase:?}"
        );
        assert!(
            menu_content_progress(false, 0.6, 1.0) > 0.75,
            "content must remain coherent through the initial deflation"
        );
        assert_eq!(menu_content_progress(false, 0.2, 1.0), 0.0);
        let rounded_volume = menu_geometry_phase(false, 0.21);
        assert!(
            rounded_volume.width > 0.35,
            "the terminal body must contract continuously into the anchor: {rounded_volume:?}"
        );
        assert!(
            44.0 + (250.0 - 44.0) * rounded_volume.width
                > 44.0 + (104.0 - 44.0) * rounded_volume.height,
            "the terminal body must stay smooth rather than forming a vertical leaf in physical dimensions: {rounded_volume:?}"
        );
    }

    #[test]
    fn menu_content_materializes_early_and_is_sharp_by_settle() {
        let birth = menu_content_progress(true, 0.35, 0.25);
        assert!(
            birth > 0.02 && birth < 0.08,
            "rows must begin as a faint smudge after the blank birth phase: {birth}"
        );
        let mid = menu_content_progress(true, 0.55, 0.55);
        assert!(
            (0.55..0.70).contains(&mid),
            "rows must remain visibly soft at mid-flight: {mid}"
        );
        let settle = menu_content_progress(true, 1.0, 0.92);
        assert!(
            settle > 0.99,
            "rows must be effectively sharp when the shape settles: {settle}"
        );
        assert!(menu_content_blur(birth) > 13.0);
        assert!((7.0..8.0).contains(&menu_content_blur(mid)));
        assert!(menu_content_blur(settle) < 0.5);
        // Sheet-timed with the grow spring: rows sharpen no earlier than the
        // reference's ~533-600 ms crisp point.
        assert!((20.0..=32.0).contains(&MENU_REVEAL_STIFFNESS));
        assert!((0.34..=0.37).contains(&menu_content_alpha(0.10)));
        assert!((0.79..=0.82).contains(&menu_content_alpha(0.62)));
        assert_eq!(menu_content_alpha(1.0), 1.0);
        assert!((0.85..=0.87).contains(&menu_content_scale(0.30)));
        assert!((0.92..=0.93).contains(&menu_content_scale(0.62)));
        assert_eq!(menu_content_scale(1.0), 1.0);
    }

    #[test]
    fn menu_surface_motion_is_smooth_and_capture_cadence_independent() {
        let merged = menu_surface_phase(true, 0.14, 0.0);
        assert_eq!(merged.anchor_presence, 0.0);
        assert_eq!(merged.glue, 0.0);
        let recoil = menu_surface_phase(true, 0.275, 0.0);
        assert_eq!(recoil.glue, 0.0);

        let early = menu_surface_phase(true, 0.40, 0.25);
        assert!(
            early.anchor_presence == 0.0,
            "the primary alone owns the anchor recoil: {early:?}"
        );
        assert_eq!(early.glue, 0.0);
        assert!(early.wobble <= 0.10);
        assert!(early.bulge <= 0.40);
        assert_eq!(early, menu_surface_phase(true, 0.40, 0.25));

        let closing = menu_surface_phase(false, 0.6, 0.68);
        assert_eq!(closing.anchor_presence, 0.0);
        assert_eq!(closing.glue, 0.0);
        assert!(closing.wobble <= 0.05);
        assert!(
            closing.bulge <= 0.30,
            "close must remain smooth: {closing:?}"
        );
    }

    #[test]
    fn menu_trigger_backdrop_unmounts_during_the_first_absorption_frame() {
        assert!((30..=40).contains(&MENU_TRIGGER_ABSORPTION_MS));
        assert_eq!(MENU_TRIGGER_RESTORE_DELAY_MS, 205);
    }

    #[test]
    fn absorbed_source_foreground_stays_readable_then_stretches_into_the_surface() {
        let source = LiquidMenuAbsorbedSource::new(
            Rect {
                x: 10.0,
                y: 20.0,
                width: 44.0,
                height: 44.0,
            },
            crate::widgets::GlassButtonSpec::glass()
                .with_icon_backplate(Color::from_rgb_u8(0, 122, 255))
                .with_content_color(Color::WHITE),
            44.0,
            "M0 0",
        );
        assert_eq!(source.rect.width, 44.0);
        assert_eq!(source.diameter, 44.0);
        assert_eq!(source.icon_path, "M0 0");

        let source = menu_absorbed_visual_phase(0.0, 0.0);
        assert_eq!(source.foreground_alpha, 1.0);
        assert_eq!(source.backdrop_alpha, 0.0);

        // Fully readable past ~100ms of the open (reference "…" is crisp
        // at 66-100ms; the grow spring puts appear ~0.25 there).
        let crisp = menu_absorbed_visual_phase(0.20, 0.20);
        assert_eq!(crisp.foreground_alpha, 1.0);
        assert_eq!(crisp.backdrop_alpha, 0.0);
        assert_eq!(crisp.foreground_blur, 0.0);
        assert!((0.76..=0.78).contains(&crisp.scale_x));
        assert!((0.76..=0.78).contains(&crisp.scale_y));
        // Ghosted to the resting 0.40 once the droplet thickens over it,
        // before the melt band (path 0.45+) begins.
        let dimmed = menu_absorbed_visual_phase(0.60, 0.30);
        assert!((0.38..=0.42).contains(&dimmed.foreground_alpha));

        let melt = menu_absorbed_visual_phase(0.95, 0.90);
        assert_eq!(melt.foreground_alpha, 0.0);
        assert_eq!(melt.backdrop_alpha, 0.62);
        assert!((0.92..=0.97).contains(&melt.scale_y));
        assert!((0.87..=0.905).contains(&melt.scale_x));

        // Mid-melt: the ghost is still fading (measured 220/255 glyph-min
        // at 250ms on the 2x capture) and the backdrop smudge is rising.
        let smear = menu_absorbed_visual_phase(0.72, 0.69);
        assert!((0.94..=0.98).contains(&smear.scale_y));
        assert!((0.89..=0.91).contains(&smear.scale_x));
        assert!((0.10..=0.18).contains(&smear.foreground_alpha));
        assert!((0.36..=0.44).contains(&smear.backdrop_alpha));
        // Early growth: readable ghost, no smudge yet.
        let transition = menu_absorbed_visual_phase(0.42, 0.382);
        assert!((0.60..=0.75).contains(&transition.foreground_alpha));
        assert_eq!(transition.backdrop_alpha, 0.0);
        let settled = menu_absorbed_visual_phase(1.0, 1.0);
        assert_eq!(settled.foreground_alpha, 0.0);
        assert_eq!(settled.backdrop_alpha, 0.62);
        assert_eq!(MENU_SOURCE_FOREGROUND_HIDE_MS, 200);
        assert_eq!(MENU_SOURCE_FOREGROUND_RESTORE_DELAY_MS, 205);
    }

    #[test]
    fn liquid_menu_item_builders_preserve_the_row_contract() {
        let item = LiquidMenuItem::new("Delete")
            .icon("M0 0")
            .checked(true)
            .destructive()
            .section_start();
        assert_eq!(item.label, "Delete");
        assert_eq!(item.icon, Some("M0 0"));
        assert!(item.checked);
        assert!(item.destructive);
        assert!(item.section_start);
        assert!(!item.header);

        let header = LiquidMenuItem::header("Show");
        assert_eq!(header.label, "Show");
        assert!(header.header);
    }

    #[test]
    fn claimed_menu_gesture_streams_one_release_to_an_interactive_row() {
        let _runtime =
            cranpose_core::Runtime::new(std::sync::Arc::new(cranpose_core::DefaultScheduler));
        let gesture = LiquidMenuGesture::new();
        let items = vec![LiquidMenuItem::header("Show"), LiquidMenuItem::new("Grid")];
        gesture.item_rect(0).set(Rect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 30.0,
        });
        gesture.item_rect(1).set(Rect {
            x: 10.0,
            y: 50.0,
            width: 100.0,
            height: 40.0,
        });

        gesture.begin(Point::new(80.0, 10.0));
        gesture.claim();
        gesture.move_to(Point::new(40.0, 65.0));
        let held = gesture.snapshot();
        assert!(held.active && held.claimed);
        assert_eq!(gesture.item_at(held.position, &items), Some(1));
        assert_eq!(gesture.item_at(Point::new(40.0, 35.0), &items), None);

        gesture.release(Point::new(40.0, 65.0));
        let released = gesture.snapshot();
        assert!(!released.active);
        assert_eq!(released.release, Some((1, Point::new(40.0, 65.0))));
        // A second lift without a new press cannot synthesize another action.
        gesture.release(Point::new(40.0, 65.0));
        assert_eq!(gesture.snapshot().release, released.release);
    }

    #[test]
    fn committing_a_row_dismisses_unless_the_row_keeps_the_menu_open() {
        let taps = Rc::new(Cell::new(0usize));
        let dismissals = Rc::new(Cell::new(0usize));
        let on_item: Rc<dyn Fn(usize)> = {
            let taps = Rc::clone(&taps);
            Rc::new(move |_| taps.set(taps.get() + 1))
        };
        let on_dismiss: Rc<dyn Fn()> = {
            let dismissals = Rc::clone(&dismissals);
            Rc::new(move || dismissals.set(dismissals.get() + 1))
        };

        commit_menu_row(0, false, &on_item, &on_dismiss);
        assert_eq!(
            (taps.get(), dismissals.get()),
            (1, 1),
            "an ordinary row dismisses"
        );

        commit_menu_row(1, true, &on_item, &on_dismiss);
        assert_eq!(
            (taps.get(), dismissals.get()),
            (2, 1),
            "a row that keeps the menu open runs its action and dismisses nothing"
        );
    }

    #[test]
    fn a_row_is_committed_exactly_once_per_tap() {
        // Both ways a row can be committed — its own pointer handler, and a
        // press that slid through the menu — run this one function, so a tap
        // can dismiss at most once. Two copies of the rule are what let a
        // dropdown dismiss on top of the menu's own dismissal.
        let dismissals = Rc::new(Cell::new(0usize));
        let on_item: Rc<dyn Fn(usize)> = Rc::new(|_| {});
        let on_dismiss: Rc<dyn Fn()> = {
            let dismissals = Rc::clone(&dismissals);
            Rc::new(move || dismissals.set(dismissals.get() + 1))
        };
        commit_menu_row(0, false, &on_item, &on_dismiss);
        assert_eq!(dismissals.get(), 1);
    }
}
