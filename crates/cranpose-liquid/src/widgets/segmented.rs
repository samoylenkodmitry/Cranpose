use std::{cell::RefCell, rc::Rc};

use cranpose_animation::{Animatable, AnimationType, Easing, Lerp, spring, tween};
use cranpose_core::{MutableState, State, mutableStateOf, remember, with_current_composer};
use cranpose_macros::composable;
use cranpose_ui::{
    Modifier, PointerInputScope, SemanticsWidgetRole, SharedText, Size,
    text::{FontWeight, TextStyle},
    widgets::{Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, Row, RowSpec, Text},
};
use cranpose_ui_graphics::{Brush, Color, ColorFilter, CornerRadii, GraphicsLayer};
use cranpose_ui_layout::Alignment;

use crate::{
    glass_morph::GlassMorph,
    material::{GlassDynamics, LiquidModifierExt},
    motion::LiquidMotion,
    theme::{LiquidSegmentedStyle, liquid_segmented_style},
    widgets::content_scope::ScopeContent,
};

const TRACK_PADDING: f32 = 2.0;
const LENS_PAD: f32 = 10.0;
const TAP_SLOP: f32 = 4.0;

// iOS 27 dims a pressed segment's label to 65% over about 150 ms and lifts
// the dim as fast once the finger leaves.
const PRESSED_LABEL_DIM: f32 = 0.35;

fn label_dim_motion() -> AnimationType {
    tween(150, Easing::EaseInOut)
}

// The lens's travel to a tapped segment, fitted to the iOS 27 tap's layer
// trace: it arrives in about 350 ms with a 3% overshoot.
fn tap_travel_motion() -> AnimationType {
    spring(0.86, 207.0)
}

// The segment a finger holds down without taking the lens, and how far its
// label has dimmed.
struct SegmentHighlight {
    index: MutableState<Option<usize>>,
    dim: RefCell<Animatable<f32>>,
    dim_state: State<f32>,
}

impl SegmentHighlight {
    fn set(&self, index: Option<usize>, time: Option<u64>) {
        if let Some(index) = index {
            self.index.set(Some(index));
        }
        crate::motion::retarget(
            &mut self.dim.borrow_mut(),
            f32::from(index.is_some()),
            label_dim_motion(),
            time,
        );
    }

    // Reads the dim only for the highlighted label, so a dimming label
    // repaints alone.
    fn label_alpha(&self, index: usize) -> f32 {
        if self.index.get() == Some(index) {
            1.0 - PRESSED_LABEL_DIM * self.dim_state.get()
        } else {
            1.0
        }
    }
}

#[composable]
fn remember_segment_highlight() -> Rc<SegmentHighlight> {
    with_current_composer(|composer| {
        let runtime = composer.runtime_handle();
        composer
            .remember(move || {
                let dim = Animatable::new(0.0, runtime);
                let dim_state = dim.state();
                Rc::new(SegmentHighlight {
                    index: mutableStateOf(None),
                    dim: RefCell::new(dim),
                    dim_state,
                })
            })
            .with(Rc::clone)
    })
}

fn segment_lens_left(pointer_x: f32, segment_width: f32, count: usize) -> f32 {
    (pointer_x - segment_width * 0.5).clamp(0.0, segment_width * (count.saturating_sub(1)) as f32)
}

fn segmented_lens_reading(
    lens_axis: &crate::motion::LiquidDragAxis,
    pressed: bool,
    selected: usize,
    segment_width: f32,
    count: usize,
) -> usize {
    let selected_x = segment_width * selected as f32;
    let lens_x = lens_axis.value();
    crate::motion::liquid_visual_index(
        selected,
        lens_x,
        segment_width,
        count,
        crate::motion::liquid_axis_owns_visual_selection(
            pressed,
            lens_x,
            selected_x,
            segment_width,
        ),
    )
}

fn segmented_lens_base_size(segment_width: f32, height: f32, progress: f32) -> Size {
    let progress = progress.clamp(-0.1, 1.2);
    let rest_h = height - TRACK_PADDING * 2.0;
    Size::new(
        segment_width - TRACK_PADDING * 2.0 + 24.0 * progress,
        rest_h + 16.0 * progress,
    )
}

enum LiquidSegment {
    /// A label the control draws in its style.
    Label(SharedText),
    /// Content the caller draws, and what accessibility announces for it.
    Content {
        description: String,
        content: Rc<dyn Fn(bool)>,
    },
}

impl LiquidSegment {
    fn description(&self) -> String {
        match self {
            Self::Label(label) => label.to_string(),
            Self::Content { description, .. } => description.clone(),
        }
    }
}

/// The scope a segmented control's content is declared in.
///
/// Each call adds one equal-width segment, in the order it is made, which is
/// also the order the indices passed to `on_select` count in.
pub struct LiquidSegmentedControlScope {
    segments: ScopeContent<LiquidSegment>,
}

impl LiquidSegmentedControlScope {
    /// A segment showing `label` in the control's
    /// [`LiquidSegmentedStyle`]: the selected one is told apart by weight
    /// and by the selected label color, never by dimming the rest.
    pub fn segment(&self, label: impl Into<String>) {
        self.segments
            .push(LiquidSegment::Label(SharedText::from(label.into())));
    }

    /// A segment the caller draws.
    ///
    /// `selected` says whether this is the active segment, so content can
    /// respond the way the built-in label's weight does. `description` is what
    /// accessibility announces, which content alone cannot supply.
    pub fn segment_content(
        &self,
        description: impl Into<String>,
        content: impl Fn(bool) + 'static,
    ) {
        self.segments.push(LiquidSegment::Content {
            description: description.into(),
            content: Rc::new(content),
        });
    }
}

fn collect_segments(content: impl FnOnce(&LiquidSegmentedControlScope)) -> Vec<LiquidSegment> {
    ScopeContent::collect(|segments| LiquidSegmentedControlScope { segments }, content)
}

/// The labels' text, worked out once for every segment of a control.
struct SegmentLabels {
    text: TextStyle,
    selected_text: TextStyle,
    /// The chosen label's color at rest and while the lens is held, when
    /// they differ.
    held: Option<(Color, Color)>,
    highlight: Rc<SegmentHighlight>,
}

impl SegmentLabels {
    fn new(style: &LiquidSegmentedStyle, highlight: Rc<SegmentHighlight>) -> Self {
        let mut text = style.text.clone();
        text.span_style.color = Some(style.label);
        let mut selected_text = style.text.clone();
        let weight = selected_text
            .span_style
            .font_weight
            .unwrap_or(FontWeight::NORMAL);
        selected_text.span_style.font_weight = Some(FontWeight((weight.0 + 100).min(900)));
        selected_text.span_style.color = Some(style.selected_label);
        Self {
            text,
            selected_text,
            held: (style.selected_label != style.label)
                .then_some((style.selected_label, style.label)),
            highlight,
        }
    }

    /// Draws `label`. The chosen label follows the indicator: as a held lens
    /// clears to glass, the label moves to the color of the others, which
    /// reads on the track.
    fn show(&self, label: &SharedText, index: usize, selected: bool, lift: State<f32>) {
        let held = self.held.filter(|_| selected);
        let text = if selected {
            &self.selected_text
        } else {
            &self.text
        };
        // Only a label that can change color carries a layer: the chosen one
        // under a held lens, and the one the finger last highlighted.
        if held.is_none() && self.highlight.index.get() != Some(index) {
            Text(label.clone(), Modifier::empty(), text.clone());
            return;
        }
        let highlight = Rc::clone(&self.highlight);
        let modifier = Modifier::empty().graphics_layer(move || {
            let alpha = highlight.label_alpha(index);
            let held = held.map(|held| (held, lift.get().clamp(0.0, 1.0)));
            let color_filter = match held {
                Some(((rest, held), lift)) if lift > 0.0 => {
                    let color = rest.lerp(&held, lift);
                    Some(ColorFilter::tint(color.with_alpha(color.a() * alpha)))
                }
                _ => (alpha < 1.0).then(|| ColorFilter::modulate(Color::WHITE.with_alpha(alpha))),
            };
            GraphicsLayer {
                color_filter,
                ..Default::default()
            }
        });
        Text(label.clone(), modifier, text.clone());
    }
}

// What a segmented control's touches drive.
struct SegmentGesture {
    axis: Rc<crate::motion::LiquidDragAxis>,
    contact: Rc<super::control_motion::ControlContactMotion>,
    highlight: Rc<SegmentHighlight>,
    pressed: MutableState<bool>,
    on_select: Rc<dyn Fn(usize)>,
    segment_width: f32,
    count: usize,
    selected: usize,
}

impl SegmentGesture {
    fn modifier(self, size: Size) -> Modifier {
        Modifier::empty().size(size).pointer_input(
            self.selected,
            move |scope: PointerInputScope| {
                crate::motion::liquid_lens_gesture(scope, self.lens_gesture())
            },
        )
    }

    fn lens_gesture(&self) -> crate::motion::LiquidLensGesture {
        let (segment_width, count, pressed) = (self.segment_width, self.count, self.pressed);
        let contact = Rc::clone(&self.contact);
        let highlight = Rc::clone(&self.highlight);
        crate::motion::LiquidLensGesture {
            axis: Rc::clone(&self.axis),
            cell_width: segment_width,
            cell_offset: 0.0,
            count,
            tap_slop: TAP_SLOP,
            drag_left: Rc::new(move |x| segment_lens_left(x, segment_width, count)),
            rest_left: Rc::new(move |index| segment_width * index as f32),
            selected: self.selected,
            on_pressed: Rc::new(move |press, time| {
                pressed.set(press.is_down());
                contact.pressed(press, time);
            }),
            on_touch: Rc::new(|_, _| {}),
            on_select: Rc::clone(&self.on_select),
            other_cell_press: crate::motion::OtherCellPress::Highlights {
                cell: Rc::new(move |index, time| highlight.set(index, time)),
                travel: tap_travel_motion(),
            },
        }
    }
}

/// A segmented control. `content` declares equal-width segments; `selected` is
/// the active index; `on_select` receives the committed index. Segments tap AND
/// swipe: dragging slides the indicator with the finger as a glass lens. The
/// look comes from [`liquid_segmented_style`].
///
/// ```rust,ignore
/// LiquidSegmentedControl(Modifier::empty().width(310.0), selected.get(), move |index| {
///     selected.set(index)
/// }, |scope| {
///     scope.segment("Receiving");
///     scope.segment("Sending");
///     scope.segment("Errored");
/// });
/// ```
#[composable]
pub fn LiquidSegmentedControl(
    modifier: Modifier,
    selected: usize,
    on_select: impl Fn(usize) + 'static,
    content: impl FnOnce(&LiquidSegmentedControlScope),
) {
    // Only this shell is generic, so every call site shares the body below.
    SegmentedTrack(
        modifier,
        selected,
        Rc::new(on_select),
        collect_segments(content),
    );
}

// The track, the lens and the cells of a segmented control.
#[composable(no_skip)]
fn SegmentedTrack(
    modifier: Modifier,
    selected: usize,
    on_select: Rc<dyn Fn(usize)>,
    segments: Vec<LiquidSegment>,
) {
    let style = liquid_segmented_style();
    let count = segments.len().max(1);
    let selected = selected.min(count - 1);
    let segments = Rc::new(segments);

    let pressed = remember(|| mutableStateOf(false)).with(|s| *s);
    let contact = super::control_motion::remember_control_contact(
        super::control_motion::ControlContactKind::Segment,
    );
    let progress = contact.states();

    let height = style.height;
    let track_fill = style.track;
    let marker_fill = style.indicator;
    let labels = Rc::new(SegmentLabels::new(&style, remember_segment_highlight()));
    let track = Modifier::empty().height(height).draw_behind(move |scope| {
        scope.draw_round_rect(Brush::solid(track_fill), CornerRadii::uniform(height * 0.5));
    });

    Box(modifier.then(track), BoxSpec::default(), move || {
        let segments = Rc::clone(&segments);
        let labels = Rc::clone(&labels);
        let on_select = Rc::clone(&on_select);
        let contact = Rc::clone(&contact);
        BoxWithConstraints(Modifier::empty(), move |scope| {
            let total_width = scope.constraints().max_width.max(1.0);
            let segment_width = total_width / count as f32;
            let selected_x = segment_width * selected as f32;
            let lens_axis =
                crate::motion::remember_liquid_follow_axis(selected_x, spring(1.0, 750.0));
            let shape =
                super::lens_motion::remember_lens_shape(super::lens_motion::LensShapeKind::Segment);
            if !pressed.get() {
                lens_axis.settle_to(selected_x, LiquidMotion::glide());
            }
            let visual_index = {
                let lens_axis = Rc::clone(&lens_axis);
                cranpose_core::derivedStateOf(move || {
                    segmented_lens_reading(
                        &lens_axis,
                        pressed.get(),
                        selected,
                        segment_width,
                        count,
                    )
                })
                .get()
            };

            let gesture = SegmentGesture {
                axis: Rc::clone(&lens_axis),
                contact: Rc::clone(&contact),
                highlight: Rc::clone(&labels.highlight),
                pressed,
                on_select: Rc::clone(&on_select),
                segment_width,
                count,
                selected,
            }
            .modifier(Size::new(total_width, height));

            let lens =
                SegmentLens::new(lens_axis, shape, progress, Size::new(segment_width, height));
            Box(lens.glass(marker_fill), BoxSpec::default(), || {});
            let row = SegmentRow {
                segments: Rc::clone(&segments),
                labels: Rc::clone(&labels),
                on_select: Rc::clone(&on_select),
                selected,
                visual_index,
                cell: lens.cell,
                material_progress: lens.material_progress,
            };
            Row(
                lens.content_layer(Modifier::empty().selectable_group(), total_width),
                RowSpec::default(),
                move || SegmentCells(&row),
            );
            Box(lens.foreground(), BoxSpec::default(), || {});
            Box(gesture, BoxSpec::default(), || {});
        });
    });
}

// The glass lens of a segmented control: where it slides, its shape and how
// far a touch has grown it and cleared it to glass.
struct SegmentLens {
    axis: Rc<crate::motion::LiquidDragAxis>,
    shape: Rc<super::lens_motion::LensShapeMotion>,
    lens_progress: State<f32>,
    material_progress: State<f32>,
    // One segment's size.
    cell: Size,
    // The node that holds the lens at its largest.
    node: Size,
}

impl SegmentLens {
    fn new(
        axis: Rc<crate::motion::LiquidDragAxis>,
        shape: Rc<super::lens_motion::LensShapeMotion>,
        (lens_progress, material_progress): (State<f32>, State<f32>),
        cell: Size,
    ) -> Self {
        let node = super::control_lens::node_size(
            segmented_lens_base_size(cell.width, cell.height, 0.0),
            segmented_lens_base_size(cell.width, cell.height, 1.0),
            LENS_PAD,
        );
        Self {
            axis,
            shape,
            lens_progress,
            material_progress,
            cell,
            node,
        }
    }

    // The lens node, centered on the selected cell, with its offset.
    fn node_modifier(&self) -> Modifier {
        let (cell, node) = (self.cell, self.node);
        Modifier::empty().required_size(node).offset(
            (cell.width - node.width) * 0.5,
            (cell.height - node.height) * 0.5,
        )
    }

    // The lens's glass, which follows the axis and grows with a touch.
    fn glass(&self, marker_fill: Color) -> Modifier {
        let (cell, node) = (self.cell, self.node);
        let (lens_progress, material_progress) = (self.lens_progress, self.material_progress);
        let layer_axis = Rc::clone(&self.axis);
        let physics_axis = Rc::clone(&self.axis);
        let shape = Rc::clone(&self.shape);
        self.node_modifier()
            .graphics_layer(move || GraphicsLayer {
                translation_x: layer_axis.value(),
                alpha: 1.0,
                ..Default::default()
            })
            .glass_effect_with(
                super::control_material::material(
                    super::control_motion::ControlContactKind::Segment,
                ),
                move || {
                    let grow = lens_progress.get().clamp(-0.1, 1.2);
                    let base_size = segmented_lens_base_size(cell.width, cell.height, grow);
                    let projection = shape.projection(physics_axis.value());
                    let material = material_progress.get().max(0.0);
                    GlassDynamics {
                        optical_projection: Some(projection),
                        resting_tint: Some(marker_fill),
                        morph: Some(GlassMorph {
                            node_size: (node.width, node.height),
                            primary: (
                                node.width * 0.5,
                                node.height * 0.5,
                                base_size.width,
                                base_size.height,
                                -1.0,
                            ),
                            shapes: Vec::new(),
                            glue: 0.0,
                            wobble_amplitude: 0.0,
                            wobble_phase: 0.0,
                            bulge_amplitude: 0.0,
                            bulge_direction: 0.0,
                            ellipse_blend: 0.0,
                            capsule_smoothing_dp: 0.0,
                            deformation: None,
                            zoom_anchor: (0.0, 0.0),
                            tail: None,
                        }),
                        ..super::control_material::dynamics(
                            super::control_motion::ControlContactKind::Segment,
                            grow,
                            material,
                        )
                    }
                },
            )
    }

    // `modifier` with the layer that refracts the labels under the lens, on
    // a row `total_width` wide.
    fn content_layer(&self, modifier: Modifier, total_width: f32) -> Modifier {
        let cell = self.cell;
        let (lens_progress, material_progress) = (self.lens_progress, self.material_progress);
        let axis = Rc::clone(&self.axis);
        let shape = Rc::clone(&self.shape);
        modifier.graphics_layer(move || {
            let position = axis.value();
            GraphicsLayer {
                render_effect: super::control_material::content_effect(
                    Size::new(total_width, cell.height),
                    (position + cell.width * 0.5, cell.height * 0.5),
                    segmented_lens_base_size(cell.width, cell.height, lens_progress.get()),
                    shape.projection(position),
                    material_progress.get().max(0.0),
                ),
                ..Default::default()
            }
        })
    }

    // The lens's foreground, drawn over the labels.
    fn foreground(&self) -> Modifier {
        let (cell, node) = (self.cell, self.node);
        let (lens_progress, material_progress) = (self.lens_progress, self.material_progress);
        let axis = Rc::clone(&self.axis);
        let shape = Rc::clone(&self.shape);
        self.node_modifier().graphics_layer(move || {
            let position = axis.value();
            GraphicsLayer {
                translation_x: position,
                backdrop_effect: super::control_material::foreground_effect(
                    node,
                    segmented_lens_base_size(cell.width, cell.height, lens_progress.get()),
                    shape.projection(position),
                    material_progress.get().max(0.0),
                ),
                ..Default::default()
            }
        })
    }
}

// What the row of a segmented control's cells reads.
struct SegmentRow {
    segments: Rc<Vec<LiquidSegment>>,
    labels: Rc<SegmentLabels>,
    on_select: Rc<dyn Fn(usize)>,
    selected: usize,
    // The segment the lens shows as chosen while it moves.
    visual_index: usize,
    cell: Size,
    material_progress: State<f32>,
}

// One cell per segment: its reader semantics and its label or content.
#[composable(no_skip)]
fn SegmentCells(row: &SegmentRow) {
    let material_progress = row.material_progress;
    for (index, segment) in row.segments.iter().enumerate() {
        let is_selected = index == row.visual_index;
        let cell = Modifier::empty()
            .size(row.cell)
            .stable_semantics(super::selection::selection_semantics(
                segment.description(),
                SemanticsWidgetRole::RadioButton,
                index,
                super::selection::SelectionState::of(index, row.selected),
                Rc::clone(&row.on_select),
            ))
            .focusable();
        let (drawn, labels) = (Rc::clone(&row.segments), Rc::clone(&row.labels));
        Box(
            cell,
            BoxSpec::default().content_alignment(Alignment::CENTER),
            move || match &drawn[index] {
                LiquidSegment::Label(label) => {
                    labels.show(label, index, is_selected, material_progress);
                }
                LiquidSegment::Content { content, .. } => content(is_selected),
            },
        );
    }
}

#[cfg(test)]
#[path = "tests/segmented_tests.rs"]
mod tests;
