//! The floating glass tab bar: a capsule of tabs over live content with a
//! liquid selection blob (dual-spring stretch), plus an optional detached
//! circular accessory (the iOS 26 search button).

use crate::material::{Glass, GlassDynamics, GlassMorph, LiquidModifierExt};
use crate::motion::LiquidMotion;
use crate::theme::{liquid_colors, liquid_typography};
use cranpose_animation::animateFloatAsState;
use cranpose_macros::composable;
use cranpose_services::{default_haptics, HapticFeedback};
use cranpose_ui::text::{FontWeight, SpanStyle, TextStyle};
use cranpose_ui::widgets::{
    Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, Column, ColumnSpec, Row, RowSpec,
    Text,
};
use cranpose_ui::{Modifier, PointerEventKind, PointerInputScope, Size};
use cranpose_ui_graphics::{Brush, CornerRadii, GraphicsLayer};
use cranpose_ui_layout::{Alignment, HorizontalAlignment, VerticalAlignment};
use std::cell::RefCell;
use std::rc::Rc;

/// One tab: icon path data (24×24 viewBox) + label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiquidTab {
    pub icon: &'static str,
    pub label: &'static str,
}

impl LiquidTab {
    pub fn new(icon: &'static str, label: &'static str) -> Self {
        Self { icon, label }
    }
}

const BAR_HEIGHT: f32 = 64.0;
const BLOB_HEIGHT: f32 = 52.0;
const BLOB_MARGIN: f32 = 6.0;
/// Width allotted to each tab inside the pill.
const TAB_WIDTH: f32 = 76.0;
/// Tab icon size (the reference bar draws ~24pt glyphs over 11pt labels).
const TAB_ICON_SIZE: f32 = 24.0;
const TAB_LABEL_SIZE: f32 = 11.0;
/// The drag lens overflows the pill vertically like the reference bubble
/// (the pressed lens pokes visibly past the bar even before it moves).
/// The drag lens is markedly taller than the bar (reference ≈ 1.35×).
const TAP_SLOP: f32 = 6.0;

/// A floating glass tab bar. Place it in an overlay `Box` above scrolling
/// content — the glass lenses whatever moves beneath it. `accessory`
/// composes a detached element to the right (pass `|| {}` for none).
#[composable]
#[allow(non_snake_case)]
pub fn LiquidTabBar(
    modifier: Modifier,
    tabs: Vec<LiquidTab>,
    selected: usize,
    on_select: impl Fn(usize) + 'static,
    accessory: impl FnMut() + 'static,
) {
    let colors = liquid_colors();
    let typography = liquid_typography();
    let count = tabs.len().max(1);
    let selected = selected.min(count - 1);
    let on_select: Rc<dyn Fn(usize)> = Rc::new(on_select);
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

            // The main pill (wrapped in a stack so the drag lens can float
            // ABOVE the finished bar and magnify icons + glass together).
            // Lighter frost than a menu: the reference bar keeps the list
            // beneath readable.
            let lens_x_outer =
                cranpose_core::remember(|| cranpose_core::mutableStateOf((0.0f32, 0.0f32, 0.0f32)))
                    .with(|state| *state);
            // The stack is pinned to the pill height so the mounting lens
            // (taller than the bar) can never inflate it — an unpinned stack
            // grew on press and the centering Row shifted the whole bar down.
            Box(
                Modifier::empty().height(BAR_HEIGHT),
                BoxSpec::default(),
                move || {
                    let tabs = Rc::clone(&tabs);
                    let typography = typography.clone();
                    let on_select = Rc::clone(&on_select);
                    // The bar's edge is defined by shadow and contrast, not a
                    // bright rim stroke.
                    let pill = Modifier::empty()
                        .glass_effect(Glass::regular().blur_radius(14.0).highlight(0.55))
                        .height(BAR_HEIGHT);
                    Box(pill, BoxSpec::default(), move || {
                        let tabs = Rc::clone(&tabs);
                        let typography = typography.clone();
                        let on_select = Rc::clone(&on_select);
                        BoxWithConstraints(Modifier::empty().padding(BLOB_MARGIN), move |scope| {
                            let tabs = Rc::clone(&tabs);
                            let typography = typography.clone();
                            let on_select = Rc::clone(&on_select);
                            let constrained = scope.constraints().max_width;
                            let tab_width = if constrained.is_finite() && constrained > 1.0 {
                                (constrained / count as f32).min(TAB_WIDTH * 1.4)
                            } else {
                                TAB_WIDTH
                            };

                            // Liquid selection blob: the edge facing the target runs
                            // a stiffer spring than the one behind, so the highlight
                            // stretches like a droplet in motion and settles round —
                            // velocity carries across quick tab hops. The direction
                            // latches per hop so the springs keep their roles while
                            // settling.
                            let previous =
                                cranpose_core::remember(|| cranpose_core::mutableStateOf(selected))
                                    .with(|state| *state);
                            let moving_right =
                                cranpose_core::remember(|| cranpose_core::mutableStateOf(true))
                                    .with(|state| *state);
                            if previous.get() != selected {
                                moving_right.set(selected > previous.get());
                                previous.set(selected);
                            }
                            let (left_spec, right_spec) = if moving_right.get() {
                                (LiquidMotion::blob_trailing(), LiquidMotion::blob_leading())
                            } else {
                                (LiquidMotion::blob_leading(), LiquidMotion::blob_trailing())
                            };
                            let leading = animateFloatAsState(
                                tab_width * selected as f32,
                                left_spec,
                                "tabbar-blob-leading",
                            );
                            let trailing = animateFloatAsState(
                                tab_width * (selected + 1) as f32,
                                right_spec,
                                "tabbar-blob-trailing",
                            );

                            // Interactive lens: engages at the CURRENT selection on
                            // touch-down, chases the finger while swiping, flies to
                            // the committed tab on release and then dissolves. The
                            // gray pill hides while the lens is up.
                            let lens_drag_x = cranpose_core::remember(|| {
                                cranpose_core::mutableStateOf(Option::<f32>::None)
                            })
                            .with(|state| *state);
                            let lens_pressed =
                                cranpose_core::remember(|| cranpose_core::mutableStateOf(false))
                                    .with(|state| *state);
                            let lens_target_x = match lens_drag_x.get() {
                                // The reference lens travels PAST the bar's end
                                // cap toward the search circle (that overdrag is
                                // what brings the two glass rims into glue
                                // reach); it snaps back to a real tab on release.
                                Some(x) => (x - tab_width * 0.5)
                                    .clamp(-tab_width * 0.2, tab_width * (count as f32 - 0.45)),
                                None => tab_width * selected as f32,
                            };
                            // Finger-attached: tight chase. Released: the
                            // committed-slot flight runs the reference ~330ms
                            // glide (velocity carries across the retarget).
                            let lens_x = animateFloatAsState(
                                lens_target_x,
                                if lens_drag_x.get().is_some() {
                                    LiquidMotion::snappy()
                                } else {
                                    LiquidMotion::glide()
                                },
                                "tabbar-lens-x",
                            );
                            // Visually settled well before 1dp spring convergence: gate the
                            // dissolve at ~12dp so the optic starts fading the moment
                            // the flight reads as arrived (the reference is flat
                            // ~370ms after settle; a 1dp gate added a ~400ms armed
                            // plateau first).
                            let lens_settling = (lens_x.get() - lens_target_x).abs() > 12.0;
                            let lens_alpha_target = if lens_pressed.get()
                                || (lens_drag_x.get().is_none() && lens_settling)
                            {
                                1.0
                            } else {
                                0.0
                            };
                            // Arming is INSTANT — the reference flight starts
                            // as a full lens with the gray pill already gone
                            // (never two shapes); only the post-settle
                            // dissolve is gentle. The spring still retargets
                            // to 1 so a dissolve always starts from 1.
                            // Dissolve pace: the reference lens is optically
                            // flat ~350ms after settling (judge-measured);
                            // the old gentle spring was still glassy at 4×
                            // that.
                            let lens_alpha_anim = animateFloatAsState(
                                lens_alpha_target,
                                cranpose_animation::spring(1.0, 450.0),
                                "tabbar-lens-alpha",
                            );
                            let lens_alpha = move || {
                                if lens_alpha_target > 0.5 {
                                    1.0
                                } else {
                                    lens_alpha_anim.get()
                                }
                            };

                            // The selection pill is a plain gray fill a step darker
                            // than the bar glass (iOS systemFill), not more glass.
                            let blob_color = if colors.is_dark {
                                cranpose_ui_graphics::Color::from_rgba_u8(120, 120, 128, 84)
                            } else {
                                cranpose_ui_graphics::Color::from_rgba_u8(120, 120, 128, 44)
                            };
                            let blob = Modifier::empty()
                                .size(Size::new(tab_width, BLOB_HEIGHT))
                                .graphics_layer(move || {
                                    let lead = leading.get();
                                    let trail = trailing.get().max(lead + 1.0);
                                    GraphicsLayer {
                                        translation_x: lead,
                                        scale_x: ((trail - lead) / tab_width.max(1.0)).max(0.01),
                                        alpha: (1.0 - lens_alpha()).clamp(0.0, 1.0),
                                        transform_origin: cranpose_ui_graphics::TransformOrigin {
                                            pivot_fraction_x: 0.0,
                                            pivot_fraction_y: 0.5,
                                        },
                                        ..Default::default()
                                    }
                                })
                                .draw_behind(move |scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(blob_color),
                                        CornerRadii::uniform(BLOB_HEIGHT * 0.5),
                                    );
                                });
                            Box(blob, BoxSpec::default(), || {});

                            let cells_tabs = Rc::clone(&tabs);
                            Row(Modifier::empty(), RowSpec::default(), move || {
                                for (index, tab) in cells_tabs.iter().enumerate() {
                                    let is_selected = index == selected;
                                    // Unselected tabs use the full label color (the
                                    // reference bar draws them black, not gray).
                                    let color = if is_selected {
                                        colors.accent
                                    } else {
                                        colors.label
                                    };
                                    let label_for_semantics = tab.label;
                                    let cell = Modifier::empty()
                                        .size(Size::new(tab_width, BLOB_HEIGHT))
                                        .semantics(move |config| {
                                            config.is_button = true;
                                            config.is_clickable = true;
                                            config.content_description =
                                                Some(label_for_semantics.to_string());
                                        });
                                    let icon = tab.icon;
                                    let label = tab.label;
                                    let label_style = TextStyle {
                                        span_style: SpanStyle {
                                            color: Some(color),
                                            font_size: cranpose_ui::text::TextUnit::Sp(
                                                TAB_LABEL_SIZE,
                                            ),
                                            font_weight: Some(FontWeight::MEDIUM),
                                            ..typography.caption1.span_style.clone()
                                        },
                                        ..typography.caption1.clone()
                                    };
                                    Box(
                                        cell,
                                        BoxSpec::default().content_alignment(Alignment::CENTER),
                                        move || {
                                            let label_style = label_style.clone();
                                            Column(
                                                Modifier::empty(),
                                                ColumnSpec::default().horizontal_alignment(
                                                    HorizontalAlignment::CenterHorizontally,
                                                ),
                                                move || {
                                                    crate::icons::Icon(icon, TAB_ICON_SIZE, color);
                                                    Text(
                                                        label,
                                                        Modifier::empty(),
                                                        label_style.clone(),
                                                    );
                                                },
                                            );
                                        },
                                    );
                                }
                            });

                            // Swipe/tap surface across the whole pill interior.
                            let row_width = tab_width * count as f32;
                            let gesture = Modifier::empty()
                                .size(Size::new(row_width, BLOB_HEIGHT))
                                .pointer_input((), {
                                    let on_select = Rc::clone(&on_select);
                                    move |scope: PointerInputScope| {
                                        let on_select = Rc::clone(&on_select);
                                        async move {
                                            scope
                                                .await_pointer_event_scope(
                                                    |await_scope| async move {
                                                        let mut down_x = 0.0f32;
                                                        let mut active = false;
                                                        loop {
                                                            let event = await_scope
                                                                .await_pointer_event()
                                                                .await;
                                                            match event.kind {
                                                                PointerEventKind::Down => {
                                                                    active = true;
                                                                    down_x = event.position.x;
                                                                    lens_pressed.set(true);
                                                                    default_haptics().perform(
                                                                        HapticFeedback::Selection,
                                                                    );
                                                                    event.consume();
                                                                }
                                                                PointerEventKind::Move
                                                                    if active =>
                                                                {
                                                                    if (event.position.x - down_x)
                                                                        .abs()
                                                                        > TAP_SLOP
                                                                    {
                                                                        lens_drag_x.set(Some(
                                                                            event.position.x,
                                                                        ));
                                                                    }
                                                                    event.consume();
                                                                }
                                                                PointerEventKind::Up
                                                                | PointerEventKind::Cancel
                                                                    if active =>
                                                                {
                                                                    active = false;
                                                                    lens_pressed.set(false);
                                                                    let commit_x = lens_drag_x
                                                                        .get()
                                                                        .unwrap_or(
                                                                            event.position.x,
                                                                        );
                                                                    lens_drag_x.set(None);
                                                                    let index = ((commit_x
                                                                        / tab_width)
                                                                        .floor()
                                                                        as isize)
                                                                        .clamp(
                                                                            0,
                                                                            count as isize - 1,
                                                                        )
                                                                        as usize;
                                                                    default_haptics().perform(
                                                                        HapticFeedback::ImpactLight,
                                                                    );
                                                                    on_select(index);
                                                                    event.consume();
                                                                }
                                                                _ => {}
                                                            }
                                                        }
                                                    },
                                                )
                                                .await;
                                        }
                                    }
                                });
                            Box(gesture, BoxSpec::default(), || {});

                            // Publish the lens springs for the overlay rendered
                            // ABOVE the finished bar (outside this glass layer, so
                            // the lens magnifies icons and glass together).
                            let published = (lens_x.get(), lens_alpha(), tab_width);
                            if lens_x_outer.get() != published {
                                lens_x_outer.set(published);
                            }
                        });
                    });

                    // The lens bubble, floating above the whole pill. Its shape
                    // follows the droplet law (crate::dynamics): cruising speed
                    // stretches it along the travel axis, launch compresses it,
                    // braking swells the leading edge — orthogonal axis inverse,
                    // area conserved — and it magnifies harder in motion. The
                    // search accessory's circle joins its liquid field: drag the
                    // lens to the bar's end and the two glue through a
                    // smooth-union neck.
                    let (lens_px, lens_a, lens_tab_w) = lens_x_outer.get();
                    if lens_a > 0.01 {
                        let lens_w = lens_tab_w;
                        let lens_h = BAR_HEIGHT * 1.35;
                        // Node headroom for the deformation extremes (max axis
                        // stretch + leading bulge, max ortho swell) and rim glow.
                        let node_w = lens_w * crate::dynamics::STRETCH_MAX
                            + crate::dynamics::BULGE_MAX
                            + 20.0;
                        let node_h = lens_h / crate::dynamics::STRETCH_MIN + 16.0;
                        let node_x = BLOB_MARGIN + lens_px - (node_w - lens_w) * 0.5;
                        let pill_w = lens_tab_w * count as f32 + 2.0 * BLOB_MARGIN;
                        // Accessory circle center in lens-node-local coords.
                        let accessory_cx = pill_w + 10.0 + BAR_HEIGHT * 0.5 - node_x;
                        let accessory_cy = node_h * 0.5;
                        let dynamics = crate::dynamics::remember_liquid_dynamics();
                        let lens = Modifier::empty()
                            // required_size: the stack is pinned to BAR_HEIGHT so
                            // the taller lens can never inflate the bar; the node
                            // still measures (and draws) at its full size and the
                            // offset centers it on the pill.
                            .required_size(Size::new(node_w, node_h))
                            .offset(node_x, BLOB_MARGIN + (BLOB_HEIGHT - node_h) * 0.5)
                            .graphics_layer_value(GraphicsLayer {
                                alpha: lens_a.clamp(0.0, 1.0),
                                ..Default::default()
                            })
                            .glass_effect_with(
                                // Near-invisible construction: the bar lens is
                                // defined by refraction, not by rim strokes,
                                // milk or shadow (the reference flying lens
                                // has no drawn edge and no gray halo).
                                Glass::lens()
                                    .no_clip()
                                    .shadow(false)
                                    .lift(-0.03)
                                    .highlight(0.55)
                                    .chromatic_aberration(1.2)
                                    .displacement(32.0),
                                move || {
                                    // Droplet law: the pose integrates the ride
                                    // position on the animation clock.
                                    let pose = dynamics.update((lens_px, 0.0));
                                    // The optic COLLAPSES into the resting
                                    // pill as it fades — geometry and material
                                    // relax in lockstep (a frozen full-height
                                    // capsule that unmounts at fade-end reads
                                    // as a snap).
                                    let presence = lens_a.clamp(0.0, 1.0);
                                    let ease = presence * presence * (3.0 - 2.0 * presence);
                                    let base_h = BLOB_HEIGHT + (lens_h - BLOB_HEIGHT) * ease;
                                    let (w, h) = pose.size(lens_w, base_h);
                                    // The bar's edges physically cap the
                                    // droplet's vertical swell: an unclamped
                                    // launch compression ballooned the blob
                                    // into the content above the bar.
                                    let h = h.min(base_h);
                                    let energy = pose.energy();
                                    // Continuous-curvature read: the resting lens
                                    // is a flattened squircle, not a stadium; it
                                    // rounds toward a capsule only at speed.
                                    let radius = h * (0.42 + 0.08 * energy);
                                    // The search circle joins the liquid field only
                                    // when the lens edge actually gets within glue
                                    // reach — passing glue, not a permanent overdraw
                                    // (a far shape re-rendered by this node would
                                    // paint a spurious rim over the real button).
                                    let glue = 20.0;
                                    let edge_gap = (accessory_cx - node_w * 0.5).abs()
                                        - w * 0.5
                                        - BAR_HEIGHT * 0.5;
                                    // Join only when nearly touching: a parked lens
                                    // one cell away must not repaint the circle.
                                    let shapes = if edge_gap < 10.0 {
                                        vec![(
                                            accessory_cx,
                                            accessory_cy,
                                            BAR_HEIGHT,
                                            BAR_HEIGHT,
                                            -1.0,
                                        )]
                                    } else {
                                        Vec::new()
                                    };
                                    GlassDynamics {
                                        morph: Some(GlassMorph {
                                            node_size: (node_w, node_h),
                                            primary: (node_w * 0.5, node_h * 0.5, w, h, radius),
                                            shapes,
                                            glue,
                                            // A whisper: the reference outline
                                            // stays one smooth curve in every
                                            // frame — strong lobes read as a
                                            // lumpy peanut.
                                            wobble_amplitude: 1.1 * energy,
                                            wobble_phase: lens_px * 0.045,
                                            bulge_amplitude: (pose.bulge_amplitude * 0.5).min(4.0),
                                            bulge_direction: pose.bulge_direction,
                                        }),
                                        // The reference lens magnifies its cell hard
                                        // even while pressed-still, harder in motion.
                                        magnify_boost: 0.25 + 0.4 * energy,
                                        ..Default::default()
                                    }
                                },
                            );
                        Box(lens, BoxSpec::default(), || {});
                    }
                },
            );

            // Detached circular accessory (e.g. the search button).
            Box(Modifier::empty().width(10.0), BoxSpec::default(), || {});
            (accessory.borrow_mut())();
        },
    );
}

/// The standard detached accessory: a circular glass search button.
#[composable]
#[allow(non_snake_case)]
pub fn LiquidTabBarSearchAccessory(on_click: impl Fn() + 'static) {
    // The reference search circle is nearly flush with the bar height.
    crate::widgets::GlassIconButton(
        Modifier::empty(),
        crate::widgets::GlassButtonSpec::glass(),
        BAR_HEIGHT * 0.94,
        on_click,
        crate::icons::SEARCH,
    );
}
