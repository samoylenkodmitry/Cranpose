use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_animation::{Animatable, AnimationSpec, AnimationType, Easing};
use cranpose_core::{Composer, MutableState, State, mutableStateOf, remember};
use cranpose_macros::composable;
use cranpose_services::{HapticFeedback, default_haptics};
use cranpose_ui::{
    Modifier, PointerEventKind, PointerInputScope, SemanticsWidgetRole, Size,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::{Color, GraphicsLayer, Point};
use cranpose_ui_layout::Alignment;

use super::button::{GlassButtonSpec, GlassIconForeground};
use crate::{
    glass_morph::GlassMorph,
    material::{Glass, GlassDynamics, LiquidModifierExt, LiquidShape},
    motion::LiquidMotion,
    theme::{LiquidColors, liquid_colors},
    widgets::content_scope::ScopeContent,
};

const TAP_EXIT_SLOP: f32 = 12.0;

/// One action in a [`GlassIconButtonGroup`].
#[derive(Clone)]
pub struct GlassIconButtonGroupItem {
    spec: GlassButtonSpec,
    icon_path: &'static str,
    content_description: String,
    on_click: Rc<RefCell<dyn FnMut()>>,
}

impl PartialEq for GlassIconButtonGroupItem {
    fn eq(&self, other: &Self) -> bool {
        self.spec == other.spec
            && self.icon_path == other.icon_path
            && self.content_description == other.content_description
            && Rc::ptr_eq(&self.on_click, &other.on_click)
    }
}

impl GlassIconButtonGroupItem {
    /// Creates an action that draws `icon_path`, is announced as `content_description` and runs
    /// `on_click` when pressed. Its look is `GlassButtonSpec::glass`.
    pub fn new(
        icon_path: &'static str,
        content_description: impl Into<String>,
        on_click: impl FnMut() + 'static,
    ) -> Self {
        Self {
            spec: GlassButtonSpec::glass(),
            icon_path,
            content_description: content_description.into(),
            on_click: Rc::new(RefCell::new(on_click)),
        }
    }

    /// Returns the action with `spec` as its look.
    pub fn with_spec(mut self, spec: GlassButtonSpec) -> Self {
        self.spec = spec;
        self
    }
}

/// The scope a group's actions are declared in.
///
/// Each call adds one circular action, in the order it is made, which is also
/// the order they sit in left to right.
pub struct GlassIconButtonGroupScope {
    items: ScopeContent<GlassIconButtonGroupItem>,
}

impl GlassIconButtonGroupScope {
    /// An action drawing `icon_path`, announced as `content_description`.
    pub fn action(
        &self,
        icon_path: &'static str,
        content_description: impl Into<String>,
        on_click: impl FnMut() + 'static,
    ) {
        self.push(GlassIconButtonGroupItem::new(
            icon_path,
            content_description,
            on_click,
        ));
    }

    /// An action built out, for the material a particular action needs — see
    /// [`GlassIconButtonGroupItem`].
    pub fn push(&self, item: GlassIconButtonGroupItem) {
        self.items.push(item);
    }
}

fn collect_items(
    content: impl FnOnce(&GlassIconButtonGroupScope),
) -> Vec<GlassIconButtonGroupItem> {
    ScopeContent::collect(|items| GlassIconButtonGroupScope { items }, content)
}

/// Geometry and interaction material for [`GlassIconButtonGroup`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassIconButtonGroupSpec {
    diameter: f32,
    spacing: f32,
    pressed_scale: f32,
    glue_radius: f32,
}

impl GlassIconButtonGroupSpec {
    /// Creates a spec for buttons of `diameter` dp, at least 1. The other values keep their
    /// defaults.
    pub fn new(diameter: f32) -> Self {
        Self {
            diameter: diameter.max(1.0),
            ..Self::default()
        }
    }

    /// Returns the spec with `spacing` dp between neighboring buttons, at least zero.
    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing.max(0.0);
        self
    }

    /// Returns the spec with the scale to which a pressed button grows, at least 1. A value of 1
    /// means no growth.
    pub fn with_pressed_scale(mut self, pressed_scale: f32) -> Self {
        self.pressed_scale = pressed_scale.max(1.0);
        self
    }

    /// Returns the spec with the distance in dp within which the glass of neighboring buttons
    /// blends while one is pressed, at least zero.
    pub fn with_glue_radius(mut self, glue_radius: f32) -> Self {
        self.glue_radius = glue_radius.max(0.0);
        self
    }
}

impl Default for GlassIconButtonGroupSpec {
    fn default() -> Self {
        Self {
            diameter: 44.0,
            spacing: 8.0,
            pressed_scale: 1.55,
            glue_radius: 12.0,
        }
    }
}

fn merge_glyph_alpha(merge: f32) -> f32 {
    let m = merge.clamp(0.0, 1.0);
    if m <= 0.0 {
        return 1.0;
    }
    let fade_out = 1.0 - ((m - 0.02) / 0.10).clamp(0.0, 1.0);
    let fade_in = ((m - 0.78) / 0.18).clamp(0.0, 1.0);
    fade_out.max(fade_in)
}

fn icon_group_width(count: usize, spec: GlassIconButtonGroupSpec) -> f32 {
    if count == 0 {
        0.0
    } else {
        spec.diameter * count as f32 + spec.spacing * count.saturating_sub(1) as f32
    }
}

fn icon_group_item_at(
    x: f32,
    y: f32,
    count: usize,
    spec: GlassIconButtonGroupSpec,
) -> Option<usize> {
    if !(0.0..=spec.diameter).contains(&y) {
        return None;
    }
    let pitch = spec.diameter + spec.spacing;
    let index = (x / pitch).floor() as isize;
    if index < 0 || index as usize >= count {
        return None;
    }
    let local_x = x - index as f32 * pitch;
    (local_x <= spec.diameter).then_some(index as usize)
}

fn icon_group_neighbor_shapes(
    count: usize,
    active: usize,
    spec: GlassIconButtonGroupSpec,
    pad: f32,
    center_y: f32,
) -> Vec<(f32, f32, f32, f32, f32)> {
    let pitch = spec.diameter + spec.spacing;
    let contact_diameter = spec.diameter * 0.36;
    let contact_offset = spec.diameter * 0.38;
    (0..count)
        .filter(|index| index.abs_diff(active) == 1)
        .map(|index| {
            let toward_active = if index < active { 1.0 } else { -1.0 };
            (
                pad + index as f32 * pitch + spec.diameter * 0.5 + toward_active * contact_offset,
                center_y,
                contact_diameter,
                contact_diameter,
                -1.0,
            )
        })
        .collect()
}

/// A row of circular actions whose pressed glass can join adjacent members.
/// Each member keeps its own base material and foreground; one transparent,
/// persistent interaction field supplies the shared refraction and neck.
#[composable]
pub fn GlassIconButtonGroup(
    modifier: Modifier,
    spec: GlassIconButtonGroupSpec,
    content: impl FnOnce(&GlassIconButtonGroupScope),
) {
    // Only this shell is generic, so every call site shares the body below.
    IconGroup(modifier, spec, collect_items(content));
}

// What a group remembers: its members, the members leaving it and the press.
#[derive(Clone)]
struct IconGroupState {
    live_items: Rc<RefCell<Vec<GlassIconButtonGroupItem>>>,
    // Members that left, each with its old index and how far it was pressed.
    ghosts: Rc<RefCell<Vec<(GlassIconButtonGroupItem, usize, f32)>>>,
    ghost_fade: Rc<RefCell<Animatable<f32>>>,
    active: MutableState<Option<usize>>,
    drag_x: MutableState<Option<f32>>,
    last_active: Rc<Cell<usize>>,
    press_progress: State<f32>,
    // The pressed member left the group while the press lasted.
    press_orphaned: Rc<Cell<bool>>,
}

impl IconGroupState {
    fn remember(count: usize) -> Self {
        let live_items = remember(|| Rc::new(RefCell::new(Vec::new()))).with(Rc::clone);
        let ghosts = remember(|| Rc::new(RefCell::new(Vec::new()))).with(Rc::clone);
        let ghost_fade = remember(|| {
            let runtime = cranpose_core::with_current_composer(Composer::runtime_handle);
            Rc::new(RefCell::new(Animatable::new(0.0f32, runtime)))
        })
        .with(Rc::clone);
        let active = remember(|| mutableStateOf(None::<usize>)).with(|state| *state);
        let drag_x = remember(|| mutableStateOf(None::<f32>)).with(|state| *state);
        let last_active = remember(|| Rc::new(Cell::new(0usize))).with(Rc::clone);
        if let Some(index) = active.get() {
            last_active.set(index.min(count - 1));
        }
        let press_progress = cranpose_animation::animateFloatAsState(
            if active.get().is_some() { 1.0 } else { 0.0 },
            LiquidMotion::snappy(),
            "glass-icon-group-press",
        );
        let press_orphaned = remember(|| Rc::new(Cell::new(false))).with(Rc::clone);
        Self {
            live_items,
            ghosts,
            ghost_fade,
            active,
            drag_x,
            last_active,
            press_progress,
            press_orphaned,
        }
    }

    // Takes `items` as the members. Members that left fade out as ghosts;
    // returns the ghosts' alpha.
    fn take_members(&self, items: Vec<GlassIconButtonGroupItem>) -> State<f32> {
        {
            let member_stays = |member: &GlassIconButtonGroupItem| {
                items.iter().any(|item| {
                    item.icon_path == member.icon_path
                        && item.content_description == member.content_description
                })
            };
            let previous = self.live_items.borrow();
            if !previous.is_empty() {
                let leavers: Vec<(GlassIconButtonGroupItem, usize, f32)> = previous
                    .iter()
                    .enumerate()
                    .filter(|(_, member)| !member_stays(member))
                    .map(|(index, member)| {
                        let departure = if index == self.last_active.get() {
                            self.press_orphaned.set(true);
                            self.press_progress.get().clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        (member.clone(), index, departure)
                    })
                    .collect();
                if !leavers.is_empty() {
                    *self.ghosts.borrow_mut() = leavers;
                    let mut fade = self.ghost_fade.borrow_mut();
                    fade.snapTo(1.0);
                    fade.animateTo(
                        0.0,
                        AnimationType::Tween(AnimationSpec::tween(250, Easing::EaseIn)),
                    );
                }
            }
        }
        let ghost_alpha = self.ghost_fade.borrow().state();
        if !self.ghosts.borrow().is_empty() && ghost_alpha.get() <= 0.01 {
            self.ghosts.borrow_mut().clear();
        }
        *self.live_items.borrow_mut() = items;
        ghost_alpha
    }

    // `modifier` sized to the group, with the pointer input that presses,
    // slides across and releases its members.
    fn gesture(&self, modifier: Modifier, spec: GlassIconButtonGroupSpec, size: Size) -> Modifier {
        let count = self.live_items.borrow().len();
        let (active, drag_x) = (self.active, self.drag_x);
        let gesture_items = Rc::clone(&self.live_items);
        let gesture_press_orphaned = Rc::clone(&self.press_orphaned);
        modifier
            .size(size)
            .pointer_input(count, move |scope: PointerInputScope| {
                let gesture_items = Rc::clone(&gesture_items);
                let gesture_press_orphaned = Rc::clone(&gesture_press_orphaned);
                async move {
                    scope
                        .await_pointer_event_scope(|await_scope| async move {
                            let mut down_index = None::<usize>;
                            loop {
                                let event = await_scope.await_pointer_event().await;
                                match event.kind {
                                    PointerEventKind::Down if down_index.is_none() => {
                                        down_index = icon_group_item_at(
                                            event.position.x,
                                            event.position.y,
                                            gesture_items.borrow().len(),
                                            spec,
                                        );
                                        if let Some(index) = down_index {
                                            active.set(Some(index));
                                            drag_x.set(Some(event.position.x));
                                            gesture_press_orphaned.set(false);
                                            default_haptics().perform(HapticFeedback::Selection);
                                            event.consume();
                                        }
                                    }
                                    PointerEventKind::Move if down_index.is_some() => {
                                        drag_x.set(Some(event.position.x));
                                        event.consume();
                                    }
                                    PointerEventKind::Up => {
                                        let pressed_index = down_index.take();
                                        active.set(None);
                                        drag_x.set(None);
                                        if let Some(index) = pressed_index {
                                            release_member(
                                                &gesture_items,
                                                index,
                                                event.position,
                                                spec,
                                            );
                                            event.consume();
                                        }
                                    }
                                    PointerEventKind::Cancel if down_index.take().is_some() => {
                                        active.set(None);
                                        drag_x.set(None);
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
}

// Runs the action of the member at `index` when the finger lifts at
// `position` within reach of the group.
fn release_member(
    items: &RefCell<Vec<GlassIconButtonGroupItem>>,
    index: usize,
    position: Point,
    spec: GlassIconButtonGroupSpec,
) {
    let count = items.borrow().len();
    let released_inside = (-TAP_EXIT_SLOP..=spec.diameter + TAP_EXIT_SLOP).contains(&position.y)
        && (-TAP_EXIT_SLOP..=icon_group_width(count, spec) + TAP_EXIT_SLOP).contains(&position.x);
    if released_inside {
        let on_click = items
            .borrow()
            .get(index)
            .map(|item| Rc::clone(&item.on_click));
        if let Some(on_click) = on_click {
            default_haptics().perform(HapticFeedback::ImpactLight);
            (on_click.borrow_mut())();
        }
    }
}

// The group's body below its generic shell.
#[composable(no_skip)]
fn IconGroup(
    modifier: Modifier,
    spec: GlassIconButtonGroupSpec,
    items: Vec<GlassIconButtonGroupItem>,
) {
    let count = items.len();
    if count == 0 {
        return;
    }

    let colors = liquid_colors();
    let state = IconGroupState::remember(count);
    let ghost_alpha = state.take_members(items);
    let render_items = state.live_items.borrow().clone();

    let ghost_slots = state
        .ghosts
        .borrow()
        .iter()
        .map(|(_, index, _)| index + 1)
        .max()
        .unwrap_or(0);
    let layout_count = count.max(ghost_slots);
    let merge = if layout_count > count {
        (1.0 - ghost_alpha.get().clamp(0.0, 1.0)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let full_width = icon_group_width(layout_count, spec);
    let settled_width = icon_group_width(count, spec);
    let width = settled_width + (full_width - settled_width) * (1.0 - merge);
    let gesture = state.gesture(modifier, spec, Size::new(width, spec.diameter));
    let frame = IconGroupFrame {
        state,
        spec,
        colors,
        render_items,
        width,
        merge,
        ghost_alpha,
        ghost_trailing_shift: width - full_width,
    };
    Box(gesture, BoxSpec::default(), move || IconGroupLayers(&frame));
}

// What one composition of a group draws from.
struct IconGroupFrame {
    state: IconGroupState,
    spec: GlassIconButtonGroupSpec,
    colors: LiquidColors,
    render_items: Vec<GlassIconButtonGroupItem>,
    width: f32,
    // How far the group has closed the room of members that left.
    merge: f32,
    ghost_alpha: State<f32>,
    ghost_trailing_shift: f32,
}

impl IconGroupFrame {
    fn pitch(&self) -> f32 {
        self.spec.diameter + self.spec.spacing
    }

    // The pressed member, or the one pressed last.
    fn active_index(&self) -> usize {
        self.state
            .last_active
            .get()
            .min(self.render_items.len() - 1)
    }

    // The shared glass under the members: the pressed member's grown lens and
    // the necks it draws toward its neighbors.
    fn shared_glass(&self) -> Modifier {
        let spec = self.spec;
        let (count, pitch) = (self.render_items.len(), self.pitch());
        let pad = spec.glue_radius + spec.diameter * (spec.pressed_scale - 1.0) * 0.5 + 4.0;
        let node_width = self.width + pad * 2.0;
        let node_height = spec.diameter * spec.pressed_scale + pad * 2.0;
        let shared_progress = self.state.press_progress;
        let shared_orphaned = Rc::clone(&self.state.press_orphaned);
        let shared_last_active = Rc::clone(&self.state.last_active);
        let drag_x = self.state.drag_x;
        let union_tint = self
            .render_items
            .get(self.active_index())
            .and_then(|item| {
                item.spec.resolve_material(
                    &self.colors,
                    item.spec.icon_color(&self.colors),
                    spec.diameter,
                )
            })
            .and_then(|material| material.tint)
            .map_or(Color(1.0, 1.0, 1.0, 0.035), |tint| tint.with_alpha(0.85));
        Modifier::empty()
            .required_size(Size::new(node_width, node_height))
            .offset(-pad, (spec.diameter - node_height) * 0.5)
            .glass_effect_with(
                Glass::regular()
                    .blur_radius(0.0)
                    .tint(union_tint)
                    .lift(0.0)
                    .highlight(0.48)
                    .shadow(false)
                    .no_clip(),
                move || {
                    let progress = if shared_orphaned.get() {
                        0.0
                    } else {
                        shared_progress.get().clamp(0.0, 1.0)
                    };
                    let active_index = shared_last_active.get().min(count - 1);
                    let center_y = node_height * 0.5;
                    let rest_center = active_index as f32 * pitch + spec.diameter * 0.5;
                    let ridden = drag_x.get().map_or(rest_center, |x| {
                        x.clamp(rest_center - pitch * 0.35, rest_center + pitch * 0.35)
                    });
                    let center_x = pad + rest_center + (ridden - rest_center) * progress;
                    let diameter = spec.diameter * (1.0 + (spec.pressed_scale - 1.0) * progress);
                    let touch = drag_x.get().map(|x| {
                        (
                            pad + x.clamp(0.0, node_width - 2.0 * pad),
                            center_y,
                            progress,
                        )
                    });
                    GlassDynamics {
                        activity: Some(progress),
                        touch,
                        morph: Some(GlassMorph {
                            node_size: (node_width, node_height),
                            primary: (center_x, center_y, diameter, diameter, -1.0),
                            shapes: icon_group_neighbor_shapes(
                                count,
                                active_index,
                                spec,
                                pad,
                                center_y,
                            ),
                            glue: spec.glue_radius * progress,
                            ..Default::default()
                        }),
                        ..Default::default()
                    }
                },
            )
    }
}

// The shared glass, then each member's surface, foreground and the ghosts.
#[composable(no_skip)]
fn IconGroupLayers(frame: &IconGroupFrame) {
    Box(frame.shared_glass(), BoxSpec::default(), || {});
    IconGroupSurfaces(frame);
    IconGroupForegrounds(frame);
    IconGroupGhosts(frame);
}

// Each member's own glass, which grows and rides the finger while pressed.
#[composable(no_skip)]
fn IconGroupSurfaces(frame: &IconGroupFrame) {
    let (spec, pitch, active_index) = (frame.spec, frame.pitch(), frame.active_index());
    let drag_x = frame.state.drag_x;
    for (index, item) in frame.render_items.iter().enumerate() {
        let x = index as f32 * pitch;
        let surface_progress = frame.state.press_progress;
        let item_is_active = index == active_index;
        let scale_orphaned = Rc::clone(&frame.state.press_orphaned);
        let outer = Modifier::empty()
            .size(Size::new(spec.diameter, spec.diameter))
            .offset(x, 0.0);
        let rest_center = index as f32 * pitch + spec.diameter * 0.5;
        let scale_layer = Modifier::empty().graphics_layer(move || {
            let progress = if item_is_active && !scale_orphaned.get() {
                surface_progress.get().clamp(0.0, 1.0)
            } else {
                0.0
            };
            let scale = 1.0 + (spec.pressed_scale - 1.0) * progress;
            let ridden = if item_is_active {
                drag_x
                    .get()
                    .map(|x| x.clamp(rest_center - pitch * 0.35, rest_center + pitch * 0.35))
                    .unwrap_or(rest_center)
            } else {
                rest_center
            };
            GraphicsLayer {
                translation_x: (ridden - rest_center) * progress,
                scale_x: scale,
                scale_y: scale,
                ..Default::default()
            }
        });
        let mut surface = Modifier::empty().size(Size::new(spec.diameter, spec.diameter));
        if let Some(material) = item.spec.resolve_material(
            &frame.colors,
            item.spec.icon_color(&frame.colors),
            spec.diameter,
        ) {
            let surface_progress = frame.state.press_progress;
            let dynamics_orphaned = Rc::clone(&frame.state.press_orphaned);
            surface = surface.glass_effect_with(material.shape(LiquidShape::Circle), move || {
                let progress = if dynamics_orphaned.get() {
                    0.0
                } else {
                    surface_progress.get().clamp(0.0, 1.0)
                };
                GlassDynamics {
                    highlight_boost: if item_is_active { 0.60 * progress } else { 0.0 },
                    saturation_boost: if item_is_active { 1.6 * progress } else { 0.0 },
                    tint_alpha_multiplier: item_is_active.then_some(1.0 + 0.85 * progress),
                    ..Default::default()
                }
            });
        }
        Box(outer, BoxSpec::default(), move || {
            let surface = surface.clone();
            Box(scale_layer.clone(), BoxSpec::default(), move || {
                Box(surface.clone(), BoxSpec::default(), || {});
            });
        });
    }
}

// Each member's glyph and reader semantics; the pressed member's glyph fades
// while its glass grows.
#[composable(no_skip)]
fn IconGroupForegrounds(frame: &IconGroupFrame) {
    let (spec, pitch, active_index) = (frame.spec, frame.pitch(), frame.active_index());
    for (index, item) in frame.render_items.iter().enumerate() {
        let x = index as f32 * pitch;
        let item_is_active = index == active_index;
        let foreground_progress = frame.state.press_progress;
        let foreground_orphaned = Rc::clone(&frame.state.press_orphaned);
        let foreground_spec = item.spec.clone();
        let icon_path = item.icon_path;
        let description = item.content_description.clone();
        let merge_alpha = merge_glyph_alpha(frame.merge);
        let foreground = Modifier::empty()
            .size(Size::new(spec.diameter, spec.diameter))
            .offset(x, 0.0)
            .graphics_layer(move || GraphicsLayer {
                alpha: merge_alpha
                    * if item_is_active && !foreground_orphaned.get() {
                        1.0 - foreground_progress.get().clamp(0.0, 1.0)
                    } else {
                        1.0
                    },
                ..Default::default()
            })
            .stable_semantics(move |config| {
                config.role = Some(SemanticsWidgetRole::Button);
                config.is_clickable = true;
                config.content_description = Some(description.clone());
            })
            .focusable();
        Box(
            foreground,
            BoxSpec::default().content_alignment(Alignment::CENTER),
            move || {
                GlassIconForeground(foreground_spec.clone(), spec.diameter, icon_path);
            },
        );
    }
}

// Members that left the group, fading where they stood.
#[composable(no_skip)]
fn IconGroupGhosts(frame: &IconGroupFrame) {
    let (spec, pitch, colors) = (frame.spec, frame.pitch(), frame.colors);
    for (ghost, ghost_index, departure) in frame.state.ghosts.borrow().iter() {
        let x = *ghost_index as f32 * pitch + frame.ghost_trailing_shift;
        let fade = frame.ghost_alpha;
        let departure = *departure;
        let ghost_layer = Modifier::empty()
            .size(Size::new(spec.diameter, spec.diameter))
            .offset(x, 0.0)
            .graphics_layer(move || {
                let alpha = fade.get().clamp(0.0, 1.0);
                let scale = 1.0 + (spec.pressed_scale - 1.0) * departure * (0.55 + 0.45 * alpha);
                GraphicsLayer {
                    alpha,
                    scale_x: scale,
                    scale_y: scale,
                    render_effect: (departure > 0.05 && alpha < 0.98).then(|| {
                        cranpose_ui_graphics::RenderEffect::blur((1.0 - alpha) * 9.0 * departure)
                    }),
                    ..Default::default()
                }
            });
        let ghost_spec = ghost.spec.clone();
        let ghost_icon = ghost.icon_path;
        let surface = ghost
            .spec
            .resolve_material(&colors, ghost.spec.icon_color(&colors), spec.diameter)
            .map(|material| {
                Modifier::empty()
                    .size(Size::new(spec.diameter, spec.diameter))
                    .glass_effect_with(material.shape(LiquidShape::Circle), move || GlassDynamics {
                        highlight_boost: 0.60 * departure,
                        saturation_boost: 1.6 * departure,
                        tint_alpha_multiplier: Some(1.0 + 0.85 * departure),
                        ..Default::default()
                    })
            });
        let glyph_alpha = 1.0 - departure;
        Box(
            ghost_layer,
            BoxSpec::default().content_alignment(Alignment::CENTER),
            move || {
                if let Some(surface) = surface.clone() {
                    Box(surface, BoxSpec::default(), || {});
                }
                let glyph_layer = Modifier::empty().graphics_layer(move || GraphicsLayer {
                    alpha: glyph_alpha,
                    ..Default::default()
                });
                let glyph_spec = ghost_spec.clone();
                Box(
                    glyph_layer,
                    BoxSpec::default().content_alignment(Alignment::CENTER),
                    move || {
                        GlassIconForeground(glyph_spec.clone(), spec.diameter, ghost_icon);
                    },
                );
            },
        );
    }
}

#[cfg(test)]
#[path = "tests/glass_icon_button_group_tests.rs"]
mod tests;
