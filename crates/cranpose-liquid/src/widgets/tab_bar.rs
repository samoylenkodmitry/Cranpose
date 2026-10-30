use std::{cell::RefCell, rc::Rc};

use cranpose_macros::composable;
use cranpose_ui::{
    Color, Modifier, PointerInputScope, Size,
    widgets::{Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, Painter, Row, RowSpec},
};
use cranpose_ui_layout::VerticalAlignment;

use super::{
    tab_cells::{TabCells, TabCellsSpec},
    tab_flight_lens::{TabFlightGeometry, TabFlightNode, tab_lens_base_size},
    tab_lens_drawing::TabLensDrawing,
};
use crate::{
    material::LiquidModifierExt,
    motion::{LiquidMotion, Press},
    theme::{LiquidColors, liquid_colors, liquid_typography},
    widgets::{content_scope::ScopeContent, glass_surface::surface_material},
};

/// Visual treatment for a tab icon.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LiquidTabIconStyle {
    /// A tinted symbol in the shared icon frame.
    #[default]
    Plain,
    /// The icon drawn as an application badge.
    AppBadge,
}

/// Artwork displayed in a tab's shared icon frame.
#[derive(Clone, Debug, PartialEq)]
pub enum LiquidTabIcon {
    /// Vector path data in a 24×24 view box.
    Vector(&'static str),
    /// Template artwork drawn at its supplied logical size and tinted with the tab's color.
    Painter {
        /// Image or custom painter supplying the template's alpha mask.
        painter: Painter,
        /// Artwork dimensions in logical points before the tab's optical correction.
        size: Size,
    },
}

/// One tab's artwork, label, and icon treatment.
#[derive(Clone, Debug, PartialEq)]
pub struct LiquidTab {
    /// Vector or template artwork shown above the label.
    pub icon: LiquidTabIcon,
    /// Shared display and accessibility text, including runtime translations.
    pub label: cranpose_ui::SharedText,
    /// How the icon is drawn: as a plain symbol or as an application badge.
    pub icon_style: LiquidTabIconStyle,
    /// Optical correction for symbols whose path bounds do not fill the
    /// shared icon frame uniformly.
    pub icon_scale: f32,
    /// Optical displacement of the artwork inside the shared icon frame, in points.
    pub icon_offset: (f32, f32),
}

impl LiquidTab {
    /// A tab with a vector icon and static or shared runtime label.
    pub fn new(icon: &'static str, label: impl Into<cranpose_ui::SharedText>) -> Self {
        Self {
            icon: LiquidTabIcon::Vector(icon),
            label: label.into(),
            icon_style: LiquidTabIconStyle::Plain,
            icon_scale: 1.0,
            icon_offset: (0.0, 0.0),
        }
    }

    /// A tab using template artwork and its logical size.
    pub fn from_painter(
        painter: Painter,
        size: Size,
        label: impl Into<cranpose_ui::SharedText>,
    ) -> Self {
        Self {
            icon: LiquidTabIcon::Painter { painter, size },
            label: label.into(),
            icon_style: LiquidTabIconStyle::Plain,
            icon_scale: 1.0,
            icon_offset: (0.0, 0.0),
        }
    }

    /// A tab whose icon is drawn as an application badge.
    pub fn app_badge(icon: &'static str, label: impl Into<cranpose_ui::SharedText>) -> Self {
        Self {
            icon: LiquidTabIcon::Vector(icon),
            label: label.into(),
            icon_style: LiquidTabIconStyle::AppBadge,
            icon_scale: 1.0,
            icon_offset: (0.0, 0.0),
        }
    }

    /// Sets the optical scale of the artwork in the shared icon frame, from 0.5 to 1.5. A nonfinite
    /// scale becomes 1.
    pub fn with_icon_scale(mut self, scale: f32) -> Self {
        self.icon_scale = normalize_icon_scale(scale);
        self
    }

    /// Moves the artwork without changing its cell or hit bounds; nonfinite offsets become zero.
    pub fn with_icon_offset(mut self, x: f32, y: f32) -> Self {
        self.icon_offset = (
            if x.is_finite() { x } else { 0.0 },
            if y.is_finite() { y } else { 0.0 },
        );
        self
    }
}

pub(super) fn normalize_icon_scale(scale: f32) -> f32 {
    if scale.is_finite() {
        scale.clamp(0.5, 1.5)
    } else {
        1.0
    }
}

fn tab_base_content_color(colors: LiquidColors) -> Color {
    colors.label
}

pub(super) fn tab_selection_content_color(colors: LiquidColors) -> Color {
    colors.accent
}

pub(super) const BAR_HEIGHT: f32 = 62.0;
pub(super) const BLOB_HEIGHT: f32 = 54.0;
pub(super) const BLOB_MARGIN: f32 = 4.0;
const TAB_LENS_OVERLAP: f32 = 7.0;
pub(super) const TAB_WIDTH: f32 = 78.0;
pub(super) const TAB_ICON_SIZE: f32 = 32.0;
pub(super) const TAB_LABEL_SIZE: f32 = 10.0;
const TAP_SLOP: f32 = 6.0;
const ACCESSORY_GAP: f32 = 10.0;

/// Layout parameters for a [`LiquidTabBar`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiquidTabBarSpec {
    max_tab_width: f32,
}

impl LiquidTabBarSpec {
    /// Sets the maximum cell allocation; nonfinite values use the default and finite values are at least one point.
    pub fn new(max_tab_width: f32) -> Self {
        Self {
            max_tab_width: if max_tab_width.is_finite() {
                max_tab_width.max(1.0)
            } else {
                TAB_WIDTH
            },
        }
    }

    fn cell_width(self, available_width: f32, count: usize) -> f32 {
        let constrained = available_width - BLOB_MARGIN * 2.0;
        if constrained.is_finite() && constrained > 1.0 {
            (constrained / count as f32).min(self.max_tab_width)
        } else {
            self.max_tab_width
        }
    }
}

impl Default for LiquidTabBarSpec {
    fn default() -> Self {
        Self::new(TAB_WIDTH)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct TabGeometry {
    pub(super) width: f32,
    pub(super) cell_width: f32,
    pub(super) pitch: f32,
}

impl TabGeometry {
    fn new(allocation: f32, count: usize) -> Self {
        let width = allocation * count.max(1) as f32;
        let cell_width = tab_lens_rest_width(allocation).min(width);
        let pitch = if count > 1 {
            (width - cell_width) / (count - 1) as f32
        } else {
            cell_width
        };
        Self {
            width,
            cell_width,
            pitch,
        }
    }

    fn drag_left(self, pointer_x: f32, count: usize, has_accessory: bool) -> f32 {
        let last_tab = self.pitch * count.saturating_sub(1) as f32;
        let min = if has_accessory {
            -self.pitch * 0.2
        } else {
            0.0
        };
        let max = if has_accessory {
            last_tab + self.pitch * 0.55
        } else {
            last_tab
        };
        (pointer_x - self.cell_width * 0.5).clamp(min, max)
    }

    fn optical_pointer_x(self, pointer_x: f32, press: f32, travel: f32) -> f32 {
        let transform = tab_bar_transform(self.width + 2.0 * BLOB_MARGIN, press, travel);
        let center = self.width * 0.5;
        pointer_x + (pointer_x - center) * (transform.scale_x - 1.0) + transform.translation_x
    }

    fn lens_surface_position(self, position: f32, press: f32, travel: f32) -> f32 {
        let transform = tab_bar_transform(self.width + 2.0 * BLOB_MARGIN, press, travel);
        (position + self.cell_width * 0.5 - self.width * 0.5) * transform.scale_x
    }
}

/// The settled cell origin for a selected index and the distance between cell centers.
pub fn tab_lens_resting_left(selected: usize, tab_pitch: f32, count: usize) -> f32 {
    tab_pitch * selected.min(count.saturating_sub(1)) as f32
}

/// The resting selection width for one destination's allocation in the content strip.
/// Cells overlap; their pitch is the remaining strip width divided by the number of gaps.
pub fn tab_lens_rest_width(tab_allocation: f32) -> f32 {
    tab_allocation + TAB_LENS_OVERLAP
}

fn tab_lens_node_top(node_height: f32) -> f32 {
    (BAR_HEIGHT - node_height) * 0.5
}

#[derive(Clone, Copy)]
struct TabBarTouch {
    position: cranpose_core::MutableState<(f32, f32)>,
    origin: cranpose_core::MutableState<Option<f32>>,
    travel: cranpose_core::MutableState<f32>,
}

impl TabBarTouch {
    fn pressed(self, press: Press) {
        if press.is_down() {
            self.origin.set(None);
            self.travel.set(0.0);
        }
    }

    fn update(self, x: f32, y: f32, span: f32) {
        self.position.set((x + BLOB_MARGIN, y + BLOB_MARGIN));
        if let Some(origin) = self.origin.get() {
            self.travel
                .set(((x - origin) / span.max(1.0)).clamp(-1.0, 1.0));
        } else {
            self.origin.set(Some(x));
        }
    }
}

#[composable]
fn remember_tab_bar_touch() -> TabBarTouch {
    TabBarTouch {
        position: cranpose_core::remember(|| cranpose_core::mutableStateOf((0.0f32, 0.0f32)))
            .with(|state| *state),
        origin: cranpose_core::remember(|| cranpose_core::mutableStateOf(None::<f32>))
            .with(|state| *state),
        travel: cranpose_core::remember(|| cranpose_core::mutableStateOf(0.0f32))
            .with(|state| *state),
    }
}

pub(super) fn tab_bar_transform(
    width: f32,
    press: f32,
    travel: f32,
) -> cranpose_ui_graphics::GraphicsLayer {
    let press = press.max(0.0);
    let travel = travel.clamp(-1.0, 1.0);
    let rise = 14.14 / width.max(1.0) * press;
    let strain = 2.63 / width.max(1.0) * travel.abs() * press;
    cranpose_ui_graphics::GraphicsLayer {
        scale_x: 1.0 + rise + strain,
        scale_y: 1.0 + rise - strain,
        translation_x: 2.756 * travel * press,
        ..Default::default()
    }
}

fn tab_bar_accessory_gap(has_accessory: bool) -> f32 {
    if has_accessory { ACCESSORY_GAP } else { 0.0 }
}

/// The scope a tab bar's destinations are declared in.
///
/// Each call adds one destination, in the order it is made, which is also the
/// order the indices passed to `on_select` count in.
pub struct LiquidTabBarScope {
    tabs: ScopeContent<LiquidTab>,
}

impl LiquidTabBarScope {
    /// A destination showing `icon` above `label`.
    pub fn tab(&self, icon: &'static str, label: impl Into<cranpose_ui::SharedText>) {
        self.push(LiquidTab::new(icon, label));
    }

    /// A destination whose icon is drawn as an application badge.
    pub fn app_badge(&self, icon: &'static str, label: impl Into<cranpose_ui::SharedText>) {
        self.push(LiquidTab::app_badge(icon, label));
    }

    /// A destination built out, for the optical corrections a particular
    /// symbol needs — see [`LiquidTab`].
    pub fn push(&self, tab: LiquidTab) {
        self.tabs.push(tab);
    }
}

fn collect_tabs(content: impl FnOnce(&LiquidTabBarScope)) -> Vec<LiquidTab> {
    ScopeContent::collect(|tabs| LiquidTabBarScope { tabs }, content)
}

/// A unified floating glass tab bar with every destination inside one pill.
///
/// ```rust,ignore
/// LiquidTabBar(Modifier::empty(), LiquidTabBarSpec::default(), selected, on_select, |scope| {
///     scope.tab(icons::DOCUMENT, "Today");
///     scope.tab(icons::SEARCH, "Search");
/// });
/// ```
#[composable]
pub fn LiquidTabBar(
    modifier: Modifier,
    spec: LiquidTabBarSpec,
    selected: usize,
    on_select: impl Fn(usize) + 'static,
    content: impl FnOnce(&LiquidTabBarScope),
) {
    LiquidTabBarLayout(
        modifier,
        spec,
        collect_tabs(content),
        selected,
        on_select,
        false,
        || {},
    );
}

/// A floating glass tab bar with a detached accessory to its right.
#[composable]
pub fn LiquidTabBarWithAccessory(
    modifier: Modifier,
    spec: LiquidTabBarSpec,
    selected: usize,
    on_select: impl Fn(usize) + 'static,
    content: impl FnOnce(&LiquidTabBarScope),
    accessory: impl FnMut() + 'static,
) {
    LiquidTabBarLayout(
        modifier,
        spec,
        collect_tabs(content),
        selected,
        on_select,
        true,
        accessory,
    );
}

#[composable]
fn LiquidTabBarLayout(
    modifier: Modifier,
    spec: LiquidTabBarSpec,
    mut tabs: Vec<LiquidTab>,
    selected: usize,
    on_select: impl Fn(usize) + 'static,
    has_accessory: bool,
    accessory: impl FnMut() + 'static,
) {
    let colors = liquid_colors();
    let typography = liquid_typography();
    let tint_amount = crate::theme::liquid_glass_tint_amount();
    let count = tabs.len().max(1);
    let selected = selected.min(count - 1);
    let rtl = cranpose_ui::layout_direction().is_rtl();
    if rtl {
        tabs.reverse();
    }
    let selected = if rtl { count - 1 - selected } else { selected };
    let on_select: Rc<dyn Fn(usize)> =
        Rc::new(move |index| on_select(if rtl { count - 1 - index } else { index }));
    let tabs = Rc::new(tabs);
    let accessory = Rc::new(RefCell::new(accessory));

    Row(
        modifier,
        RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            let tabs = Rc::clone(&tabs);
            let typography = typography.clone();
            let on_select = Rc::clone(&on_select);
            let accessory = Rc::clone(&accessory);

            let lens_pressed = cranpose_core::rememberMutableStateOf(|| false);
            let bar_touch = remember_tab_bar_touch();
            let contact_motion = super::lens_motion::remember_tab_contact_motion();
            let bar_press = contact_motion.bar_state();
            let glow = contact_motion.glow_state();
            let local_glow_factor = contact_motion.local_glow_factor_state();
            let travel = cranpose_animation::animateFloatAsState(
                bar_touch.travel.get(),
                cranpose_animation::spring(0.85, 650.0),
                "tabbar-travel",
            );
            let motion = remember_tab_bar_motion(bar_press, travel, contact_motion.lens_state());
            cranpose_ui::ProvideLayoutDirection(cranpose_ui::LayoutDirection::Ltr, || {
                Box(
                    Modifier::empty().height(BAR_HEIGHT),
                    BoxSpec::default(),
                    move || {
                        let tabs = Rc::clone(&tabs);
                        let typography = typography.clone();
                        let on_select = Rc::clone(&on_select);
                        let contact_motion = Rc::clone(&contact_motion);
                        let motion = motion.clone();
                        // The lens and the cells take this measure's width, never an older one.
                        BoxWithConstraints(Modifier::empty(), move |scope| {
                            let on_select = Rc::clone(&on_select);
                            let semantic_selection = Rc::clone(&on_select);
                            let contact_motion = Rc::clone(&contact_motion);
                            let cells = TabGeometry::new(
                                spec.cell_width(scope.constraints().max_width, count),
                                count,
                            );
                            let resting_lens_x =
                                tab_lens_resting_left(selected, cells.pitch, count);
                            let visual_index = {
                                let motion = motion.clone();
                                cranpose_core::derivedStateOf(move || {
                                    motion.visual_index(selected, cells, count, lens_pressed.get())
                                })
                                .get()
                            };
                            if bar_touch.travel.get_non_reactive().abs() > 0.001
                                && visual_index != selected
                            {
                                contact_motion.crossed_tab();
                            }
                            let pill_w = cells.width + 2.0 * BLOB_MARGIN;
                            let pill = Modifier::empty()
                                .graphics_layer(move || {
                                    tab_bar_transform(pill_w, bar_press.get(), travel.get())
                                })
                                .height(BAR_HEIGHT);
                            let published_axis = Rc::clone(&motion.axis);
                            let gesture_motion = Rc::clone(&contact_motion);
                            Box(pill, BoxSpec::default(), move || {
                                Box(
                                    Modifier::empty()
                                        .glass_effect(
                                            surface_material(colors.label).tint_amount(tint_amount),
                                        )
                                        .size(Size::new(pill_w, BAR_HEIGHT)),
                                    BoxSpec::default(),
                                    || {},
                                );
                                let lens_axis = crate::motion::remember_liquid_follow_axis(
                                    resting_lens_x,
                                    cranpose_animation::spring(0.85, 650.0),
                                );
                                if !lens_pressed.get() {
                                    lens_axis.settle_to(resting_lens_x, LiquidMotion::glide());
                                }
                                *published_axis.borrow_mut() = Some(Rc::clone(&lens_axis));
                                let gesture = TabGesture {
                                    geometry: cells,
                                    count,
                                    selected,
                                    has_accessory,
                                    lens_axis,
                                    contact_motion: Rc::clone(&gesture_motion),
                                    on_select: Rc::clone(&on_select),
                                    bar_press,
                                    travel,
                                    lens_pressed,
                                    bar_touch,
                                }
                                .modifier();
                                Box(gesture, BoxSpec::default(), || {});
                            });
                            let drawing = TabLensDrawing::remember(
                                TabLensPlacement::new(cells, resting_lens_x, colors, has_accessory),
                                motion.clone(),
                                colors,
                            );
                            let lighting = Rc::clone(&drawing);
                            super::tab_lighting::TabLighting(
                                Size::new(pill_w, BAR_HEIGHT),
                                move || lighting.transform(),
                                bar_touch.position,
                                glow,
                                local_glow_factor,
                            );
                            Box(drawing.glass_modifier(0), BoxSpec::default(), || {});
                            let (ink, lift) = (Rc::clone(&drawing), Rc::clone(&drawing));
                            TabCells(
                                Modifier::empty(),
                                Rc::clone(&tabs),
                                typography.clone(),
                                TabCellsSpec {
                                    geometry: cells,
                                    base_color: tab_base_content_color(colors),
                                    selected: visual_index,
                                    committed_selection: selected,
                                },
                                move || ink.selection(),
                                move || lift.transform(),
                                move |index| semantic_selection(index),
                            );
                            Box(drawing.glass_modifier(1), BoxSpec::default(), || {});
                        });
                    },
                );
            });

            if has_accessory {
                Box(
                    Modifier::empty().width(tab_bar_accessory_gap(true)),
                    BoxSpec::default(),
                    || {},
                );
                (accessory.borrow_mut())();
            }
        },
    );
}

struct TabGesture {
    geometry: TabGeometry,
    count: usize,
    selected: usize,
    has_accessory: bool,
    lens_axis: Rc<crate::motion::LiquidDragAxis>,
    contact_motion: Rc<super::lens_motion::TabContactMotion>,
    on_select: Rc<dyn Fn(usize)>,
    bar_press: cranpose_core::State<f32>,
    travel: cranpose_core::State<f32>,
    lens_pressed: cranpose_core::MutableState<bool>,
    bar_touch: TabBarTouch,
}

impl TabGesture {
    fn modifier(self) -> Modifier {
        let Self {
            geometry,
            count,
            selected,
            has_accessory,
            lens_axis,
            contact_motion,
            on_select,
            bar_press,
            travel,
            lens_pressed,
            bar_touch,
        } = self;
        Modifier::empty()
            .offset(BLOB_MARGIN, BLOB_MARGIN)
            .size(Size::new(geometry.width, BLOB_HEIGHT))
            .pointer_input(selected, move |scope: PointerInputScope| {
                let contact_motion = Rc::clone(&contact_motion);
                crate::motion::liquid_lens_gesture(
                    scope,
                    crate::motion::LiquidLensGesture {
                        axis: Rc::clone(&lens_axis),
                        cell_width: geometry.pitch,
                        cell_offset: (geometry.cell_width - geometry.pitch) * 0.5,
                        count,
                        tap_slop: TAP_SLOP,
                        drag_left: Rc::new(move |x| {
                            geometry.drag_left(
                                geometry.optical_pointer_x(x, bar_press.get(), travel.get()),
                                count,
                                has_accessory,
                            )
                        }),
                        rest_left: Rc::new(move |index| {
                            tab_lens_resting_left(index, geometry.pitch, count)
                        }),
                        selected,
                        on_pressed: Rc::new(move |press, time| {
                            lens_pressed.set(press.is_down());
                            contact_motion.pressed(press, time);
                            bar_touch.pressed(press);
                        }),
                        on_touch: Rc::new(move |x, y| {
                            bar_touch.update(
                                geometry.optical_pointer_x(x, bar_press.get(), travel.get()),
                                y,
                                geometry.pitch * count.saturating_sub(1) as f32,
                            );
                        }),
                        on_select: Rc::clone(&on_select),
                    },
                )
            })
    }
}

/// The tab bar's moving parts, read where they are drawn: a glide or a press
/// then redraws the bar each frame and recomposes it only when the tab under
/// the lens changes.
#[derive(Clone)]
pub(super) struct TabBarMotion {
    axis: Rc<RefCell<Option<Rc<crate::motion::LiquidDragAxis>>>>,
    lens_shape: Rc<super::lens_motion::LensShapeMotion>,
    pub(super) bar_press: cranpose_core::State<f32>,
    pub(super) travel: cranpose_core::State<f32>,
    lens_activity: cranpose_core::State<f32>,
}

/// Gathers the states the bar moves with, and remembers the lens's shape and
/// the slot its axis is published to.
#[composable]
fn remember_tab_bar_motion(
    bar_press: cranpose_core::State<f32>,
    travel: cranpose_core::State<f32>,
    lens_activity: cranpose_core::State<f32>,
) -> TabBarMotion {
    TabBarMotion {
        axis: cranpose_core::remember(|| Rc::new(RefCell::new(None))).with(Rc::clone),
        lens_shape: super::lens_motion::remember_lens_shape(super::lens_motion::LensShapeKind::Tab),
        bar_press,
        travel,
        lens_activity,
    }
}

/// What the lens looks like this frame.
#[derive(Clone, Copy)]
pub(super) struct TabLensFrame {
    pub(super) lens_x: f32,
    pub(super) activity: f32,
    pub(super) strain: Size,
    press: f32,
    travel: f32,
}

impl TabBarMotion {
    /// The lens's left edge, or where it rests before the bar has laid out.
    fn lens_x(&self, resting_lens_x: f32) -> f32 {
        self.axis
            .borrow()
            .as_ref()
            .map_or(resting_lens_x, |axis| axis.value())
    }

    /// The tab the lens is over, which the bar shows as selected while the lens
    /// owns the selection.
    fn visual_index(
        &self,
        selected: usize,
        cells: TabGeometry,
        count: usize,
        pressed: bool,
    ) -> usize {
        let resting_lens_x = tab_lens_resting_left(selected, cells.pitch, count);
        let lens_x = self.lens_x(resting_lens_x);
        crate::motion::liquid_visual_index(
            selected,
            lens_x,
            cells.pitch,
            count,
            crate::motion::liquid_axis_owns_visual_selection(
                pressed,
                lens_x,
                resting_lens_x,
                cells.pitch,
            ),
        )
    }

    /// Reads every moving part, so the caller redraws when any of them moves.
    /// The lens shape takes the lens's position once a frame, whichever
    /// reader draws first, and every reader sees the strain it holds.
    pub(super) fn frame(&self, cells: TabGeometry, resting_lens_x: f32) -> TabLensFrame {
        let press = self.bar_press.get();
        let travel = self.travel.get();
        let activity = self.lens_activity.get().clamp(0.0, 1.0);
        let lens_x = self.lens_x(resting_lens_x);
        let strain = self
            .lens_shape
            .sample(cells.lens_surface_position(lens_x, press, travel));
        TabLensFrame {
            lens_x,
            activity,
            strain,
            press,
            travel,
        }
    }
}

/// Where the flying lens sits in the bar: everything about it that does not
/// move with the lens.
#[derive(Clone, Copy)]
pub(super) struct TabLensPlacement {
    pub(super) cells: TabGeometry,
    pub(super) resting_lens_x: f32,
    pub(super) pill_w: f32,
    pub(super) node_size: Size,
    pub(super) node_top: f32,
    resting_tint: Color,
    pub(super) accessory_center: Option<(f32, f32)>,
}

impl TabLensPlacement {
    fn new(
        cells: TabGeometry,
        resting_lens_x: f32,
        colors: LiquidColors,
        has_accessory: bool,
    ) -> Self {
        let (lens_w, lens_h) = tab_lens_base_size(cells.cell_width, 1.0);
        let deformation_headroom =
            crate::dynamics::STRETCH_MAX.max(1.0 / crate::dynamics::STRETCH_MIN);
        let node_w = lens_w * deformation_headroom + crate::dynamics::BULGE_MAX + 20.0;
        let node_h = lens_h * deformation_headroom + crate::dynamics::BULGE_MAX + 16.0;
        let pill_w = cells.width + 2.0 * BLOB_MARGIN;
        Self {
            cells,
            resting_lens_x,
            pill_w,
            node_size: Size::new(node_w, node_h),
            node_top: tab_lens_node_top(node_h),
            resting_tint: colors.fill,
            accessory_center: has_accessory.then_some((
                pill_w + tab_bar_accessory_gap(true) + BAR_HEIGHT * 0.5,
                BAR_HEIGHT * 0.5,
            )),
        }
    }

    fn center_x(self, frame: TabLensFrame) -> f32 {
        BLOB_MARGIN + frame.lens_x + self.cells.cell_width * 0.5
    }

    pub(super) fn node(self, frame: TabLensFrame) -> TabFlightNode {
        TabFlightNode {
            origin: (
                self.center_x(frame) - self.node_size.width * 0.5,
                self.node_top,
            ),
            size: self.node_size,
        }
    }

    pub(super) fn geometry(self, frame: TabLensFrame) -> TabFlightGeometry {
        let (base_w, base_h) = tab_lens_base_size(self.cells.cell_width, frame.activity);
        TabFlightGeometry {
            center: (self.center_x(frame), BAR_HEIGHT * 0.5),
            base_size: Size::new(base_w, base_h),
            strain: frame.strain,
            lens_position: frame.lens_x,
            lens_activity: frame.activity,
            resting_tint: self.resting_tint,
            accessory_center: self.accessory_center,
        }
    }

    /// The lens node's layer: the bar's lift about the pill's center, and the
    /// node carried to the lens in the same translation.
    pub(super) fn layer(self, motion: &TabBarMotion) -> cranpose_ui_graphics::GraphicsLayer {
        let frame = motion.frame(self.cells, self.resting_lens_x);
        let mut layer = tab_bar_transform(self.pill_w, frame.press, frame.travel);
        let center_x = self.center_x(frame);
        layer.translation_x +=
            (center_x - self.pill_w * 0.5) * (layer.scale_x - 1.0) + self.node(frame).origin.0;
        layer
    }
}

/// The standard detached accessory: a circular glass search button.
#[composable]
pub fn LiquidTabBarSearchAccessory(on_click: impl Fn() + 'static) {
    crate::widgets::GlassIconButton(
        Modifier::empty().content_description(cranpose_ui::UiString::Search.resolve()),
        crate::widgets::GlassButtonSpec::glass(),
        BAR_HEIGHT * 0.94,
        on_click,
        crate::icons::SEARCH,
    );
}

#[cfg(test)]
#[path = "tests/tab_bar_tests.rs"]
mod tests;
