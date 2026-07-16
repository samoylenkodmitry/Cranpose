//! Glass buttons: capsule glass with a spring press (scale + specular boost)
//! and haptic feedback.

use crate::material::{Glass, GlassDynamics, GlassMorph, LiquidModifierExt, LiquidShape};
use crate::motion::{liquid_press_scale, LiquidMotion};
use crate::theme::{liquid_colors, liquid_typography};
use cranpose_animation::{AnimationSpec, AnimationType, Easing};
use cranpose_core::{mutableStateOf, remember};
use cranpose_macros::composable;
use cranpose_services::{default_haptics, HapticFeedback};
use cranpose_ui::rememberMutableInteractionSource;
use cranpose_ui::text::TextStyle;
use cranpose_ui::widgets::{Box, BoxSpec, Text};
use cranpose_ui::{Modifier, PointerEventKind, PointerInputScope, Size};
use cranpose_ui_graphics::{Color, GraphicsLayer};
use cranpose_ui_layout::Alignment;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const ICON_BACKPLATE_DIAMETER_RATIO: f32 = 0.50;
const ICON_BACKPLATE_GLYPH_RATIO: f32 = 0.28;

/// Visual style of a [`GlassButton`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GlassButtonStyle {
    /// Translucent glass with the label in the accent color.
    #[default]
    Glass,
    /// Accent-tinted glass with white content — the primary action.
    Prominent,
    /// No material: just the label with press feedback (toolbar/text button).
    Plain,
    /// Destructive variant of `Plain`.
    Destructive,
}

/// Configuration for [`GlassButton`] / [`GlassIconButton`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GlassButtonSpec {
    pub style: GlassButtonStyle,
    /// Overrides the material (advanced).
    pub glass: Option<Glass>,
    /// Overrides the label/icon color (defaults per style).
    pub content_color: Option<Color>,
    /// Optional inner color disc for icon buttons. The outer material remains
    /// clear glass; the disc colors only the icon's compact foreground core.
    pub icon_backplate: Option<Color>,
}

impl GlassButtonSpec {
    pub fn glass() -> Self {
        Self::default()
    }

    pub fn prominent() -> Self {
        Self {
            style: GlassButtonStyle::Prominent,
            ..Self::default()
        }
    }

    pub fn plain() -> Self {
        Self {
            style: GlassButtonStyle::Plain,
            ..Self::default()
        }
    }

    pub fn destructive() -> Self {
        Self {
            style: GlassButtonStyle::Destructive,
            ..Self::default()
        }
    }

    pub fn with_glass(mut self, glass: Glass) -> Self {
        self.glass = Some(glass);
        self
    }

    pub fn with_content_color(mut self, color: Color) -> Self {
        self.content_color = Some(color);
        self
    }

    pub fn with_icon_backplate(mut self, color: Color) -> Self {
        self.icon_backplate = Some(color);
        self
    }

    /// The label color for this style under `colors`.
    pub fn content_color(&self, colors: &crate::theme::LiquidColors) -> Color {
        if let Some(color) = self.content_color {
            return color;
        }
        match self.style {
            GlassButtonStyle::Glass => colors.accent,
            GlassButtonStyle::Prominent => colors.on_accent,
            GlassButtonStyle::Plain => colors.accent,
            GlassButtonStyle::Destructive => colors.destructive,
        }
    }

    /// The glyph color for icon buttons: the reference nav/search circles
    /// draw BLACK glyphs on plain glass (only prominent tints stay white).
    pub fn icon_color(&self, colors: &crate::theme::LiquidColors) -> Color {
        if let Some(color) = self.content_color {
            return color;
        }
        match self.style {
            GlassButtonStyle::Glass | GlassButtonStyle::Plain => colors.label,
            GlassButtonStyle::Prominent => colors.on_accent,
            GlassButtonStyle::Destructive => colors.destructive,
        }
    }

    fn resolve_material(
        &self,
        colors: &crate::theme::LiquidColors,
        foreground: Color,
    ) -> Option<Glass> {
        if let Some(glass) = &self.glass {
            return Some(if glass.foreground.is_some() {
                glass.clone()
            } else {
                glass
                    .clone()
                    .adaptive_frost(foreground, glass.adaptive_frost)
            });
        }
        match self.style {
            GlassButtonStyle::Glass => Some(Glass::regular().adaptive_frost(foreground, 0.65)),
            GlassButtonStyle::Prominent => Some(
                Glass::regular()
                    .tint(colors.accent.with_alpha(0.75))
                    .adaptive_frost(foreground, 0.65),
            ),
            GlassButtonStyle::Plain | GlassButtonStyle::Destructive => None,
        }
    }
}

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

    pub fn with_spec(mut self, spec: GlassButtonSpec) -> Self {
        self.spec = spec;
        self
    }
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
    pub fn new(diameter: f32) -> Self {
        Self {
            diameter: diameter.max(1.0),
            ..Self::default()
        }
    }

    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing.max(0.0);
        self
    }

    pub fn with_pressed_scale(mut self, pressed_scale: f32) -> Self {
        self.pressed_scale = pressed_scale.max(1.0);
        self
    }

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
            pressed_scale: 1.45,
            glue_radius: 12.0,
        }
    }
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

/// A glass button. `content` composes the label (see [`GlassButton`] with
/// [`Text`], or an [`crate::icons::Icon`] + text row).
#[composable]
#[allow(non_snake_case)]
pub fn GlassButton(
    modifier: Modifier,
    spec: GlassButtonSpec,
    on_click: impl Fn() + 'static,
    content: impl FnMut() + 'static,
) {
    let colors = liquid_colors();
    let interaction = rememberMutableInteractionSource();
    let (pressed_modifier, pressed, content_alpha) =
        liquid_press_scale(Modifier::empty(), interaction.clone(), 1.18);

    let material = spec.resolve_material(&colors, spec.content_color(&colors));
    let mut base = Modifier::empty();
    if let Some(glass) = material {
        let pressed_for_glass = pressed;
        base = base.glass_effect_with(glass, move || GlassDynamics {
            highlight_boost: if pressed_for_glass.get() { 0.85 } else { 0.0 },
            ..Default::default()
        });
    }

    let on_click = Rc::new(RefCell::new(on_click));
    let base = base
        .press_interaction_source(interaction)
        .clickable(move |_point| {
            default_haptics().perform(HapticFeedback::ImpactLight);
            (on_click.borrow_mut())();
        })
        .padding_symmetric(16.0, 10.0);

    // Touched glass turns more transparent: the label ghosts while pressed.
    let content_layer = Modifier::empty().graphics_layer(move || GraphicsLayer {
        alpha: content_alpha.get().clamp(0.0, 1.0),
        ..Default::default()
    });
    let content = Rc::new(RefCell::new(content));
    let button = base.then(modifier);
    Box(pressed_modifier, BoxSpec::default(), move || {
        let content = Rc::clone(&content);
        let content_layer = content_layer.clone();
        Box(
            button.clone(),
            BoxSpec::default().content_alignment(Alignment::CENTER),
            move || {
                let content = Rc::clone(&content);
                Box(
                    content_layer.clone(),
                    BoxSpec::default().content_alignment(Alignment::CENTER),
                    move || (content.borrow_mut())(),
                );
            },
        );
    });
}

/// Convenience text label styled for the enclosing button.
#[composable]
#[allow(non_snake_case)]
pub fn GlassButtonLabel(text: impl Into<String>, spec: GlassButtonSpec) {
    let typography = liquid_typography();
    let color = spec.content_color(&liquid_colors());
    let style = TextStyle {
        span_style: cranpose_ui::text::SpanStyle {
            color: Some(color),
            ..typography.headline.span_style.clone()
        },
        ..typography.headline.clone()
    };
    Text(text.into(), Modifier::empty(), style);
}

#[composable]
#[allow(non_snake_case)]
pub(crate) fn GlassIconForeground(spec: GlassButtonSpec, diameter: f32, icon_path: &'static str) {
    let colors = liquid_colors();
    let icon_color = spec.icon_color(&colors);
    if let Some(backplate) = spec.icon_backplate {
        let backplate_diameter = diameter * ICON_BACKPLATE_DIAMETER_RATIO;
        Box(
            Modifier::empty()
                .size(Size::new(backplate_diameter, backplate_diameter))
                .draw_behind(move |scope| {
                    scope.draw_circle(
                        cranpose_ui_graphics::Brush::solid(backplate),
                        cranpose_ui_graphics::Point::new(
                            backplate_diameter * 0.5,
                            backplate_diameter * 0.5,
                        ),
                        backplate_diameter * 0.5,
                    );
                }),
            BoxSpec::default().content_alignment(Alignment::CENTER),
            move || {
                crate::icons::Icon(icon_path, diameter * ICON_BACKPLATE_GLYPH_RATIO, icon_color);
            },
        );
    } else {
        crate::icons::Icon(icon_path, diameter * 0.5, icon_color);
    }
}

/// A circular glass icon button (44dp target).
#[composable]
#[allow(non_snake_case)]
pub fn GlassIconButton(
    modifier: Modifier,
    spec: GlassButtonSpec,
    diameter: f32,
    on_click: impl Fn() + 'static,
    icon_path: &'static str,
) {
    GlassIconButtonWithForegroundAlpha(modifier, spec, diameter, 1.0, on_click, icon_path);
}

#[composable]
#[allow(non_snake_case)]
pub(crate) fn GlassIconButtonWithForegroundAlpha(
    modifier: Modifier,
    spec: GlassButtonSpec,
    diameter: f32,
    foreground_alpha: f32,
    on_click: impl Fn() + 'static,
    icon_path: &'static str,
) {
    let colors = liquid_colors();
    let interaction = rememberMutableInteractionSource();
    let (pressed_modifier, pressed, content_alpha) =
        liquid_press_scale(Modifier::empty(), interaction.clone(), 1.20);

    let material = spec
        .resolve_material(&colors, spec.icon_color(&colors))
        .map(|glass| glass.shape(LiquidShape::Circle));
    let mut base = Modifier::empty();
    if let Some(glass) = material {
        let pressed_for_glass = pressed;
        base = base.glass_effect_with(glass, move || GlassDynamics {
            highlight_boost: if pressed_for_glass.get() { 0.85 } else { 0.0 },
            ..Default::default()
        });
    }

    let on_click = Rc::new(RefCell::new(on_click));
    let base = base
        .press_interaction_source(interaction)
        .clickable(move |_point| {
            default_haptics().perform(HapticFeedback::ImpactLight);
            (on_click.borrow_mut())();
        })
        .size(Size::new(diameter, diameter));

    // The reference press: the icon ghosts while the glass lifts.
    let content_layer = Modifier::empty().graphics_layer(move || GraphicsLayer {
        alpha: content_alpha.get().clamp(0.0, 1.0) * foreground_alpha.clamp(0.0, 1.0),
        ..Default::default()
    });
    let button = base.then(modifier);
    Box(pressed_modifier, BoxSpec::default(), move || {
        let foreground_spec = spec.clone();
        let content_layer = content_layer.clone();
        Box(
            button.clone(),
            BoxSpec::default().content_alignment(Alignment::CENTER),
            move || {
                let foreground_spec = foreground_spec.clone();
                Box(
                    content_layer.clone(),
                    BoxSpec::default().content_alignment(Alignment::CENTER),
                    move || GlassIconForeground(foreground_spec.clone(), diameter, icon_path),
                );
            },
        );
    });
}

/// A row of circular actions whose pressed glass can join adjacent members.
/// Each member keeps its own base material and foreground; one transparent,
/// persistent interaction field supplies the shared refraction and neck.
#[composable]
#[allow(non_snake_case)]
pub fn GlassIconButtonGroup(
    modifier: Modifier,
    spec: GlassIconButtonGroupSpec,
    items: Vec<GlassIconButtonGroupItem>,
) {
    let count = items.len();
    if count == 0 {
        return;
    }

    let colors = liquid_colors();
    let live_items: Rc<RefCell<Vec<GlassIconButtonGroupItem>>> =
        remember(|| Rc::new(RefCell::new(Vec::new()))).with(Rc::clone);
    // A member that leaves the group dissolves in place instead of popping:
    // the reference confirm disc turns translucent over the surviving "…"
    // and is gone ~250 ms later (touched-up-state frames f_0350..f_0370).
    let ghosts: Rc<RefCell<Vec<(GlassIconButtonGroupItem, usize)>>> =
        remember(|| Rc::new(RefCell::new(Vec::new()))).with(Rc::clone);
    let ghost_fade = remember(|| {
        let runtime = cranpose_core::with_current_composer(|composer| composer.runtime_handle());
        Rc::new(RefCell::new(cranpose_animation::Animatable::new(
            0.0f32, runtime,
        )))
    })
    .with(Rc::clone);
    {
        let previous = live_items.borrow();
        if !previous.is_empty() && *previous != items {
            let leavers: Vec<(GlassIconButtonGroupItem, usize)> = previous
                .iter()
                .enumerate()
                .filter(|(_, member)| !items.contains(member))
                .map(|(index, member)| (member.clone(), index))
                .collect();
            if !leavers.is_empty() {
                *ghosts.borrow_mut() = leavers;
                let mut fade = ghost_fade.borrow_mut();
                fade.snapTo(1.0);
                fade.animateTo(
                    0.0,
                    AnimationType::Tween(AnimationSpec::tween(250, Easing::EaseIn)),
                );
            }
        }
    }
    let ghost_alpha = ghost_fade.borrow().state();
    if !ghosts.borrow().is_empty() && ghost_alpha.get() <= 0.01 {
        ghosts.borrow_mut().clear();
    }
    *live_items.borrow_mut() = items;
    let render_items = live_items.borrow().clone();

    let active = remember(|| mutableStateOf(None::<usize>)).with(|state| *state);
    // The held disc rides the finger toward a neighbor (the reference press
    // drifts and necks into the adjacent circle); None between gestures.
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

    let width = icon_group_width(count, spec);
    let gesture_items = Rc::clone(&live_items);
    let gesture = Modifier::empty()
        .size(Size::new(width, spec.diameter))
        .pointer_input(count, move |scope: PointerInputScope| {
            let gesture_items = Rc::clone(&gesture_items);
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
                                        let released_index = icon_group_item_at(
                                            event.position.x,
                                            event.position.y,
                                            gesture_items.borrow().len(),
                                            spec,
                                        );
                                        if released_index == Some(index) {
                                            let on_click = gesture_items
                                                .borrow()
                                                .get(index)
                                                .map(|item| Rc::clone(&item.on_click));
                                            if let Some(on_click) = on_click {
                                                default_haptics()
                                                    .perform(HapticFeedback::ImpactLight);
                                                (on_click.borrow_mut())();
                                            }
                                        }
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
        .then(modifier);

    Box(gesture, BoxSpec::default(), move || {
        let pitch = spec.diameter + spec.spacing;
        let active_index = last_active.get().min(count - 1);

        // The union field owns the connection and shared refraction. It sits
        // behind the item faces, and while pressed it carries the ACTIVE
        // item's tint so the neck toward a neighbor flows in that material
        // (the reference bridge is the confirm action's cyan, not plain
        // glass); the tint alpha rides press activity, so the resting field
        // stays a whisper.
        let pad = spec.glue_radius + spec.diameter * (spec.pressed_scale - 1.0) * 0.5 + 4.0;
        let node_width = width + pad * 2.0;
        let node_height = spec.diameter * spec.pressed_scale + pad * 2.0;
        let shared_progress = press_progress;
        let shared_last_active = Rc::clone(&last_active);
        let union_tint = render_items
            .get(active_index)
            .and_then(|item| {
                item.spec
                    .resolve_material(&colors, item.spec.icon_color(&colors))
            })
            .and_then(|material| material.tint)
            .map(|tint| tint.with_alpha(0.45))
            .unwrap_or(Color::WHITE.with_alpha(0.035));
        let shared = Modifier::empty()
            .required_size(Size::new(node_width, node_height))
            .offset(-pad, (spec.diameter - node_height) * 0.5)
            .glass_effect_with(
                // Regular variant: the tint covers the whole union field
                // uniformly, so the thin neck carries the material — the
                // lens variant's interior ramp fades tint exactly there.
                Glass::regular()
                    .blur_radius(0.0)
                    .tint(union_tint)
                    .lift(0.0)
                    .highlight(0.48)
                    .shadow(false)
                    .no_clip(),
                move || {
                    let progress = shared_progress.get().clamp(0.0, 1.0);
                    let active_index = shared_last_active.get().min(count - 1);
                    let center_y = node_height * 0.5;
                    let rest_center = active_index as f32 * pitch + spec.diameter * 0.5;
                    // Ride the finger toward a neighbor, one pitch at most:
                    // approaching the next circle necks the union field.
                    // Half a pitch keeps the neighbor distinct: the disc
                    // leans and the glue stretches a thin neck instead of
                    // the shapes overlapping outright.
                    let ridden = drag_x
                        .get()
                        .map(|x| x.clamp(rest_center - pitch * 0.35, rest_center + pitch * 0.35))
                        .unwrap_or(rest_center);
                    let center_x = pad + rest_center + (ridden - rest_center) * progress;
                    let diameter = spec.diameter * (1.0 + (spec.pressed_scale - 1.0) * progress);
                    // The reference press concentrates saturation + a soft
                    // gradient light under the FINGER (touched-up-state
                    // recording), riding press activity.
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
            );
        Box(shared, BoxSpec::default(), || {});

        // Independent base surfaces preserve each member's style and tint.
        for (index, item) in render_items.iter().enumerate() {
            let x = index as f32 * pitch;
            let surface_progress = press_progress;
            let item_is_active = index == active_index;
            let outer = Modifier::empty()
                .size(Size::new(spec.diameter, spec.diameter))
                .offset(x, 0.0);
            let rest_center = index as f32 * pitch + spec.diameter * 0.5;
            let scale_layer = Modifier::empty().graphics_layer(move || {
                let progress = if item_is_active {
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
            if let Some(material) = item
                .spec
                .resolve_material(&colors, item.spec.icon_color(&colors))
            {
                let surface_progress = press_progress;
                surface =
                    surface.glass_effect_with(material.shape(LiquidShape::Circle), move || {
                        let progress = surface_progress.get().clamp(0.0, 1.0);
                        GlassDynamics {
                            highlight_boost: if item_is_active { 0.22 * progress } else { 0.0 },
                            saturation_boost: if item_is_active { 0.55 * progress } else { 0.0 },
                            tint_alpha_multiplier: item_is_active.then_some(1.0 + 0.22 * progress),
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

        // The active foreground is absorbed into the rising material. Other
        // members stay crisp and retain their independent ownership.
        for (index, item) in render_items.iter().enumerate() {
            let x = index as f32 * pitch;
            let item_is_active = index == active_index;
            let foreground_progress = press_progress;
            let foreground_spec = item.spec.clone();
            let icon_path = item.icon_path;
            let description = item.content_description.clone();
            let foreground = Modifier::empty()
                .size(Size::new(spec.diameter, spec.diameter))
                .offset(x, 0.0)
                .graphics_layer(move || GraphicsLayer {
                    alpha: if item_is_active {
                        1.0 - foreground_progress.get().clamp(0.0, 1.0)
                    } else {
                        1.0
                    },
                    ..Default::default()
                })
                .semantics(move |config| {
                    config.is_button = true;
                    config.is_clickable = true;
                    config.content_description = Some(description.clone());
                });
            Box(
                foreground,
                BoxSpec::default().content_alignment(Alignment::CENTER),
                move || {
                    GlassIconForeground(foreground_spec.clone(), spec.diameter, icon_path);
                },
            );
        }

        // Departed members: the same surface + glyph at their old slot,
        // riding one shared fade to transparent. No gestures, no union
        // membership — a purely optical afterimage.
        for (ghost, ghost_index) in ghosts.borrow().iter() {
            let x = *ghost_index as f32 * pitch;
            let fade = ghost_alpha;
            let ghost_layer = Modifier::empty()
                .size(Size::new(spec.diameter, spec.diameter))
                .offset(x, 0.0)
                .graphics_layer(move || GraphicsLayer {
                    alpha: fade.get().clamp(0.0, 1.0),
                    ..Default::default()
                });
            let ghost_spec = ghost.spec.clone();
            let ghost_icon = ghost.icon_path;
            let surface = ghost
                .spec
                .resolve_material(&colors, ghost.spec.icon_color(&colors))
                .map(|material| {
                    Modifier::empty()
                        .size(Size::new(spec.diameter, spec.diameter))
                        .glass_effect(material.shape(LiquidShape::Circle))
                });
            Box(
                ghost_layer,
                BoxSpec::default().content_alignment(Alignment::CENTER),
                move || {
                    if let Some(surface) = surface.clone() {
                        Box(surface, BoxSpec::default(), || {});
                    }
                    GlassIconForeground(ghost_spec.clone(), spec.diameter, ghost_icon);
                },
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_backplate_colors_only_the_compact_foreground_core() {
        let blue = Color::from_rgb_u8(0, 122, 255);
        let spec = GlassButtonSpec::glass()
            .with_icon_backplate(blue)
            .with_content_color(Color::WHITE);
        assert_eq!(spec.style, GlassButtonStyle::Glass);
        assert_eq!(spec.icon_backplate, Some(blue));
        assert_eq!(spec.content_color, Some(Color::WHITE));
        assert!(spec.glass.is_none());
        assert!((0.49..=0.51).contains(&ICON_BACKPLATE_DIAMETER_RATIO));
        assert!((0.27..=0.29).contains(&ICON_BACKPLATE_GLYPH_RATIO));

        let colors = crate::theme::LiquidColors::light(blue);
        let material = GlassButtonSpec::glass()
            .resolve_material(&colors, colors.label)
            .expect("glass button material");
        assert_eq!(material.tint, None);
        assert_eq!(material.resolve(&colors).tint, colors.glass_tint);
    }

    #[test]
    fn neutral_button_tint_comes_from_the_theme_not_foreground_polarity() {
        let accent = Color::from_rgb_u8(0, 122, 255);
        for colors in [
            crate::theme::LiquidColors::light(accent),
            crate::theme::LiquidColors::dark(accent),
        ] {
            let material = GlassButtonSpec::glass()
                .resolve_material(&colors, colors.label)
                .expect("glass button material");
            assert_eq!(material.tint, None);
            assert_eq!(material.resolve(&colors).tint, colors.glass_tint);
        }
    }

    #[test]
    fn icon_button_group_builders_and_hit_regions_preserve_member_gaps() {
        let spec = GlassIconButtonGroupSpec::new(44.0)
            .with_spacing(8.0)
            .with_pressed_scale(1.2)
            .with_glue_radius(12.0);
        assert_eq!(icon_group_width(2, spec), 96.0);
        assert_eq!(icon_group_item_at(22.0, 22.0, 2, spec), Some(0));
        assert_eq!(icon_group_item_at(48.0, 22.0, 2, spec), None);
        assert_eq!(icon_group_item_at(74.0, 22.0, 2, spec), Some(1));
        assert_eq!(icon_group_item_at(22.0, 50.0, 2, spec), None);

        let shapes = icon_group_neighbor_shapes(4, 2, spec, 16.0, 38.0);
        assert_eq!(shapes.len(), 2);
        assert!((shapes[0].0 - (16.0 + 52.0 + 22.0 + 44.0 * 0.38)).abs() < 1e-5);
        assert!((shapes[1].0 - (16.0 + 156.0 + 22.0 - 44.0 * 0.38)).abs() < 1e-5);
        assert_eq!(shapes[0].2, 44.0 * 0.36);

        let item = GlassIconButtonGroupItem::new("M0 0", "Confirm", || {})
            .with_spec(GlassButtonSpec::prominent());
        assert_eq!(item.content_description, "Confirm");
        assert_eq!(item.spec.style, GlassButtonStyle::Prominent);
    }
}
