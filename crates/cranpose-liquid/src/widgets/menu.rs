//! A popup menu that morphs out of its anchor: the glass bubble springs from
//! the anchor corner while its items fade in (the WWDC "Show" menu).

use crate::material::{
    Glass, GlassDynamics, GlassMorph, GlassShadow, LiquidModifierExt, LiquidShape,
};
use crate::theme::{liquid_colors, liquid_typography};
use cranpose_core::{mutableStateOf, remember, MutableState, SideEffect};
use cranpose_foundation::PointerId;
use cranpose_macros::composable;
use cranpose_ui::text::{FontWeight, SpanStyle, TextStyle, TextUnit};
use cranpose_ui::widgets::{
    Box, BoxSpec, Column, ColumnSpec, PopupDismissable, Row, RowSpec, Text,
};
use cranpose_ui::{
    rememberMutableInteractionSource, Modifier, PointerEventKind, PointerInputScope,
    PressInteractionPress, Size,
};
use cranpose_ui_graphics::{Brush, Color, CornerRadii, GraphicsLayer, Point, Rect, RenderEffect};
use cranpose_ui_layout::VerticalAlignment;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

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

const MENU_WIDTH: f32 = 250.0;
const MENU_RADIUS: f32 = 32.0;
const MENU_GROW_DELAY: f32 = 0.050;
const MENU_OVERSHOOT_SCALE: f32 = 0.30;
const MENU_GROW_STIFFNESS: f32 = 60.0;
const MENU_REVEAL_STIFFNESS: f32 = 200.0;
const MENU_WIDTH_EASE_POWER: f32 = 4.5;
const MENU_HEIGHT_EASE_POWER: f32 = 18.0;
const MENU_HEIGHT_OVERSHOOT: f32 = 0.15;
const MENU_HEIGHT_OVERSHOOT_END: f32 = 0.52;
const MENU_VERTICAL_REBOUND: f32 = 14.0;
const MENU_VERTICAL_REBOUND_END: f32 = 0.70;
const MENU_SOURCE_HEIGHT_RATIO: f32 = 0.80;
const MENU_SOURCE_TARGET_Y_PROGRESS: f32 = 0.76;
/// How far the card's top edge sits below the anchor's top: the settled menu
/// swallows the anchor button ENTIRELY (the reference "…" disappears under
/// the glass, reading as a smudge; only mid-flight does its bump ride the
/// droplet edge).
const ANCHOR_OVERLAP: f32 = 0.0;
const ROW_PADDING_X: f32 = 20.0;
const ROW_PADDING_Y: f32 = 9.25;
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
const MENU_SOURCE_FOREGROUND_HIDE_MS: u64 = 5;
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
pub fn remember_liquid_menu_gesture() -> LiquidMenuGesture {
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
    let overshoot_phase = ((path - 0.50) / 0.50).clamp(0.0, 1.0);
    let overshoot = 0.040 * (std::f32::consts::PI * overshoot_phase).sin().max(0.0);
    let height_overshoot_phase = (path / MENU_HEIGHT_OVERSHOOT_END).clamp(0.0, 1.0);
    let height_overshoot = MENU_HEIGHT_OVERSHOOT
        * (std::f32::consts::PI * height_overshoot_phase)
            .sin()
            .max(0.0);
    MenuGeometryPhase {
        path,
        width: 1.0 - (1.0 - path).powf(MENU_WIDTH_EASE_POWER) + overshoot,
        height: 1.0 - (1.0 - path).powf(MENU_HEIGHT_EASE_POWER) + height_overshoot,
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
    let mut primary = if expanded && appear < MENU_GROW_DELAY {
        interpolate_menu_shape(anchor, source, phase.width, phase.height)
    } else {
        let start = if expanded { source } else { anchor };
        interpolate_menu_shape(start, target, phase.width, phase.height)
    };
    if expanded {
        primary.center_y += menu_vertical_rebound(phase.path);
    }
    let blob_radius = primary.height * 0.5;
    let squareness = smoothstep(0.68, 1.0, phase.path);
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
    0.42 * smoothstep(0.08, 0.28, path) * (1.0 - smoothstep(0.72, 0.96, path))
}

fn menu_content_progress(expanded: bool, appear: f32, reveal: f32) -> f32 {
    if expanded {
        smoothstep(0.17, 0.82, reveal)
    } else {
        ((appear - 0.45) / 0.55).clamp(0.0, 1.0)
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
    let handoff = smoothstep(0.0, 0.035, appear);
    let readable_alpha = 1.0 + (0.40 - 1.0) * handoff;
    MenuAbsorbedVisualPhase {
        foreground_alpha: readable_alpha * (1.0 - smoothstep(0.16, 0.36, path)),
        backdrop_alpha: smoothstep(0.16, 0.36, path),
        foreground_blur: 4.0 * smoothstep(0.16, 0.36, path),
        scale_x: base_scale * (1.0 + 0.20 * stretch),
        scale_y: base_scale * (1.0 + 0.933 * stretch),
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

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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
        crate::motion::liquid_press_scale(Modifier::empty(), interaction.clone(), 1.12);
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
            let interaction = interaction.clone();
            let on_open = Rc::clone(&on_open);
            move |scope: PointerInputScope| {
                let gesture = gesture.clone();
                let gate = Rc::clone(&gate);
                let interaction = interaction.clone();
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
/// taps outside dismiss via `on_dismiss`; item taps call `on_item` with the
/// index then dismiss.
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
    absorbed: Vec<LiquidMenuAbsorbedSource>,
    items: Vec<LiquidMenuItem>,
    gesture: LiquidMenuGesture,
    on_item: impl Fn(usize) + 'static,
    on_dismiss: impl Fn() + 'static,
) {
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
    let on_item: Rc<dyn Fn(usize)> = Rc::new(on_item);
    let on_dismiss: Rc<dyn Fn()> = Rc::new(on_dismiss);
    let gesture_snapshot = gesture.snapshot();
    let gesture_hover = (gesture_snapshot.active && gesture_snapshot.claimed)
        .then(|| gesture.item_at(gesture_snapshot.position, &items))
        .flatten();
    let handled_release = remember(|| Rc::new(Cell::new(0u64))).with(Rc::clone);
    if let Some((sequence, point)) = gesture_snapshot.release {
        if handled_release.get() != sequence {
            handled_release.set(sequence);
            if let Some(index) = gesture.item_at(point, &items) {
                let on_item = Rc::clone(&on_item);
                let on_dismiss = Rc::clone(&on_dismiss);
                SideEffect(move || {
                    on_item(index);
                    on_dismiss();
                });
            }
        }
    }

    // The droplet spring: opening uses the bouncy morph spring (visible size
    // overshoot, the reference menu swells a few percent past its final width
    // and relaxes); closing is a faster, non-bouncy suck-back.
    // Open ≈330ms press→crisp with a soft overshoot (timed against the
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
    // Right-align the card under the anchor (menus morph out of trailing
    // buttons), staying on-screen for anchors near the right edge. The host
    // renders the outside-tap scrim (PopupDismissable). While collapsing the
    // scrim must not re-fire the caller's dismiss.
    let scrim_dismiss = Rc::clone(&on_dismiss);
    let scrim_active = expanded;
    PopupDismissable(
        anchor,
        Point::new(anchor.width - MENU_WIDTH, 0.0),
        move || {
            if scrim_active {
                scrim_dismiss()
            }
        },
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
                let anchor_center = (MENU_WIDTH - anchor.width * 0.5, anchor.height * 0.5);
                let anchor_shape = MenuShape::capsule(
                    anchor_center.0,
                    anchor_center.1,
                    anchor.width,
                    anchor.height,
                );
                let node_origin = Point::new(anchor.x + anchor.width - MENU_WIDTH, anchor.y);
                let absorbed_shapes: Vec<MenuShape> = absorbed
                    .iter()
                    .filter_map(|source| MenuShape::from_window_rect(source.rect, node_origin))
                    .collect();
                let morph_size = Rc::clone(&node_size);
                // Muted vibrancy: the absorbed button must read as a soft
                // smudge beneath the glass, not a hot saturated orb.
                let glass = Glass::regular()
                    .shape(LiquidShape::RoundedRect(MENU_RADIUS))
                    .blur_radius(8.0)
                    .saturation(1.55)
                    .lift(0.58)
                    .highlight(0.08)
                    .shadow_style(GlassShadow::new(
                        Color::BLACK.with_alpha(if colors.is_dark { 0.18 } else { 0.075 }),
                        26.0,
                        8.0,
                        0.0,
                    ))
                    .no_clip();
                let card = Modifier::empty()
                    .report_size(Rc::clone(&node_size))
                    .glass_effect_with(glass, move || {
                        let size = morph_size.get();
                        let menu_h = (size.height - anchor_zone).max(24.0);
                        // Pillowy settled corners (the reference menu's radius
                        // is ~0.26 of its height — far rounder than a desktop
                        // popup).
                        let settle_radius = (menu_h * 0.32).clamp(26.0, MENU_RADIUS);
                        let target = MenuShape {
                            center_x: MENU_WIDTH * 0.5,
                            center_y: anchor_zone + menu_h * 0.5,
                            width: MENU_WIDTH,
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
                        let glue = surface.glue;
                        GlassDynamics {
                            activity: Some(t.clamp(0.0, 1.0)),
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
                            }),
                            ..Default::default()
                        }
                    })
                    .width(MENU_WIDTH);

                let has_checks = items.iter().any(|item| item.checked);
                // Finger/pointer sliding through the menu highlights the row
                // under it (release selects) — the iOS drag-through-menu.
                let hovered = remember(|| mutableStateOf(Option::<usize>::None)).with(|s| *s);
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
                        Column(Modifier::empty().fill_max_width(), ColumnSpec::default(), {
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
                                let rows_wrap =
                                    Modifier::empty().fill_max_width().graphics_layer(move || {
                                        GraphicsLayer {
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
                                        }
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
                                                    for (index, item) in items.iter().enumerate() {
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
                                                        menu_item_row(
                                                            index,
                                                            item,
                                                            &typography,
                                                            has_checks,
                                                            colors,
                                                            hovered,
                                                            gesture_hover,
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
                        });
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
        cranpose_ui_graphics::Color::from_rgba_u8(120, 120, 128, 72)
    } else {
        cranpose_ui_graphics::Color::from_rgba_u8(120, 120, 128, 48)
    };
    let row_label = item.label.clone();
    let row = Modifier::empty()
        .fill_max_width()
        .report_window_rect(rect_sink)
        .semantics(move |config| {
            config.is_button = true;
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
                                        on_item(index);
                                        on_dismiss();
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
            if is_hovered {
                scope.draw_round_rect(Brush::solid(highlight), CornerRadii::uniform(14.0));
            }
        })
        .padding_symmetric(ROW_PADDING_X, ROW_PADDING_Y);

    let label = item.label.clone();
    let icon = item.icon;
    let checked = item.checked;
    let typography = typography.clone();
    Row(
        row,
        RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            let label = label.clone();
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
            Text(label, Modifier::empty().weight(1.0), style);
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn menu_geometry_merges_sources_and_matches_the_measured_growth_contour() {
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
        assert!(
            (94.0..=98.0).contains(&merged.width) && (75.0..=79.0).contains(&merged.height),
            "+33ms must be one smooth aggregate source droplet: {merged:?}"
        );
        assert!(
            (199.0..=204.0).contains(&merged.center_x) && (43.0..=46.0).contains(&merged.center_y),
            "the aggregate source must move toward the panel on both axes: {merged:?}"
        );

        for (label, appear, width, height) in [
            ("+54ms", 0.070_208, 108.0..=113.0, 84.0..=89.0),
            ("+75ms", 0.124_188, 139.0..=147.0, 96.0..=102.0),
            ("+100ms", 0.199_019, 174.0..=182.0, 103.0..=109.0),
            ("+130ms", 0.296_780, 205.0..=216.0, 106.0..=110.0),
            ("+165ms", 0.412_956, 228.0..=238.0, 104.0..=109.0),
            ("+205ms", 0.539_174, 241.0..=249.0, 102.0..=106.0),
        ] {
            let shape = pose(appear);
            assert!(
                width.contains(&shape.width) && height.contains(&shape.height),
                "{label} contour mismatch: {shape:?}"
            );
        }

        let swell = pose(0.701_903);
        assert!(
            (252.0..=259.0).contains(&swell.width) && (102.0..=106.0).contains(&swell.height),
            "the broad body must overshoot horizontally without inflating vertically: {swell:?}"
        );
        assert!((0.40..=0.43).contains(&menu_ellipse_blend(0.5)));
        assert_eq!(menu_ellipse_blend(0.0), 0.0);
        assert_eq!(menu_ellipse_blend(1.0), 0.0);

        let overshoot = pose(1.08);
        assert!((250.0..=254.0).contains(&overshoot.width));
        assert!((104.0..=106.0).contains(&overshoot.height));
        assert_eq!(MENU_RADIUS, 32.0);
        assert!((0.045..=0.055).contains(&MENU_GROW_DELAY));
        assert!((59.0..=61.0).contains(&MENU_GROW_STIFFNESS));
    }

    #[test]
    fn menu_open_spring_reaches_the_measured_geometry_phase_at_130ms() {
        let (appear, _) =
            cranpose_animation::advance_spring(0.0, 0.0, 1.0, 0.78, MENU_GROW_STIFFNESS, 0.130);
        assert!(
            (0.29..=0.30).contains(&appear),
            "130ms spring phase must preserve the measured contour mapping: {appear}"
        );
    }

    #[test]
    fn menu_body_and_content_share_the_measured_vertical_rebound() {
        let anchor = MenuShape::capsule(228.0, 22.0, 44.0, 44.0);
        let absorbed = [MenuShape::capsule(176.0, 22.0, 44.0, 44.0)];
        let target = MenuShape {
            center_x: 125.0,
            center_y: 52.0,
            width: 250.0,
            height: 104.0,
            radius: 32.0,
        };

        for (label, appear, expected_offset) in [
            ("+67ms", 0.070_208, 3.0..=6.0),
            ("+100ms", 0.199_019, 12.0..=16.0),
            ("+130ms", 0.296_780, 11.0..=15.0),
            ("+165ms", 0.412_956, 8.0..=12.0),
            ("+205ms", 0.539_174, 4.0..=8.0),
            ("+265ms", 0.701_903, -1.0..=2.0),
        ] {
            let geometry = menu_morph_geometry(true, appear, anchor, &absorbed, target);
            let offset = geometry.primary.center_y - target.center_y;
            assert!(
                expected_offset.contains(&offset),
                "{label} vertical rebound mismatch: {geometry:?}"
            );
        }
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
            menu_content_progress(false, 0.6, 1.0) < 0.3,
            "content must disappear with the contracting surface"
        );
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
        assert!((190.0..=210.0).contains(&MENU_REVEAL_STIFFNESS));
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

        let crisp = menu_absorbed_visual_phase(0.08, 0.15);
        assert!((0.38..=0.42).contains(&crisp.foreground_alpha));
        assert_eq!(crisp.backdrop_alpha, 0.0);
        assert_eq!(crisp.foreground_blur, 0.0);
        assert!((0.81..=0.85).contains(&crisp.scale_x));
        assert!((0.81..=0.85).contains(&crisp.scale_y));

        let melt = menu_absorbed_visual_phase(0.54, 0.52);
        assert_eq!(melt.foreground_alpha, 0.0);
        assert_eq!(melt.backdrop_alpha, 1.0);
        assert!((1.36..=1.42).contains(&melt.scale_y));
        assert!((0.87..=0.90).contains(&melt.scale_x));

        let smear = menu_absorbed_visual_phase(0.72, 0.69);
        assert!((1.42..=1.46).contains(&smear.scale_y));
        assert!((0.89..=0.91).contains(&smear.scale_x));
        assert_eq!(smear.foreground_alpha, 0.0);
        assert_eq!(smear.backdrop_alpha, 1.0);
        let transition = menu_absorbed_visual_phase(0.42, 0.382);
        assert_eq!(transition.foreground_alpha, 0.0);
        assert_eq!(transition.backdrop_alpha, 1.0);
        let settled = menu_absorbed_visual_phase(1.0, 1.0);
        assert_eq!(settled.foreground_alpha, 0.0);
        assert_eq!(settled.backdrop_alpha, 1.0);
        assert_eq!(MENU_SOURCE_FOREGROUND_HIDE_MS, 5);
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
}
