//! Liquid UI showcase: the full glass component set over colorful scrolling
//! content — buttons, toggle, slider, segmented control, chips, list cards,
//! the floating tab bar with its liquid selection blob, the morphing menu,
//! and a materials lab with live lens parameters.

#![allow(non_snake_case)]

use cranpose::liquid::prelude::*;
use cranpose::text::{SpanStyle, TextStyle, TextUnit};
use cranpose::widgets::{
    Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, Column, ColumnSpec, Row, RowSpec,
    Text,
};
use cranpose::{
    composable, mutableStateOf, remember, Brush, Color, CornerRadii, GraphicsLayer, Modifier,
    Point, PointerEventKind, PointerInputScope, Rect, ScrollState, Size,
};
use cranpose_foundation::{SemanticsConfiguration, SemanticsWidgetRole};
use cranpose_ui::{Alignment, HorizontalAlignment, VerticalAlignment};
use std::cell::RefCell;

thread_local! {
    static TAB_SWIPE_REFERENCE_PAGE_OVERRIDE: RefCell<Option<cranpose_core::MutableState<Option<usize>>>> = const { RefCell::new(None) };
}

const PAGE_PADDING: f32 = 18.0;
const LIQUID_TAB_BAR_BOTTOM_PADDING: f32 = 19.0;
const WWDC_ICON_SCALE: f32 = 0.94;
const ACCOUNT_ICON_SCALE: f32 = 0.95;
const OPTICAL_PREVIEW_HEIGHT: f32 = 256.0;
const OPTICAL_PREVIEW_LENS_SIZE: Size = Size::new(150.0, 82.0);
const OPTICAL_PREVIEW_INITIAL_Y_FRACTION: f32 = 0.28;
pub const TAB_SWIPE_REFERENCE_STAGE_WIDTH: f32 = 440.0;
pub const TAB_SWIPE_REFERENCE_STAGE_HEIGHT: f32 = 132.0;
pub const LIQUID_SCROLL_VIEWPORT_TAG: &str = "LiquidScrollViewport";

#[derive(Clone, Copy, Debug, PartialEq)]
struct TabSwipeReferenceLayout {
    size: Size,
    title: Rect,
    enroll: Rect,
    xcode: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TogglePressReferenceLayout {
    size: Size,
    track_origin: Point,
    backdrop_cap_center: Point,
    backdrop_cap_radius: f32,
    background: Color,
}

fn toggle_press_reference_layout() -> TogglePressReferenceLayout {
    TogglePressReferenceLayout {
        size: Size::new(340.0 / 3.0, 190.0 / 3.0),
        track_origin: Point::new(37.0 / 3.0, 52.0 / 3.0),
        backdrop_cap_center: Point::new(203.0 / 3.0, 94.0 / 3.0),
        backdrop_cap_radius: 77.0 / 3.0,
        background: Color::from_rgb_u8(238, 237, 245),
    }
}

fn tab_swipe_reference_layout() -> TabSwipeReferenceLayout {
    TabSwipeReferenceLayout {
        size: Size::new(440.0, 132.0),
        title: Rect {
            x: 40.0,
            y: 0.0,
            width: 360.0,
            height: 29.0,
        },
        enroll: Rect {
            x: 20.0,
            y: 33.0,
            width: 400.0,
            height: 51.0,
        },
        xcode: Rect {
            x: 32.0,
            y: 69.0,
            width: 376.0,
            height: 50.0,
        },
    }
}

fn tab_swipe_reference_glass() -> Glass {
    Glass::regular()
}

fn tab_swipe_reference_tabs() -> Vec<LiquidTab> {
    vec![
        LiquidTab::new(icons::STAR, "Discover"),
        LiquidTab::new(icons::SHOPPING_BAG, "Browse"),
        LiquidTab::app_badge(icons::APPLE, "WWDC").with_icon_scale(WWDC_ICON_SCALE),
        LiquidTab::new(icons::ACCOUNT_CIRCLE, "Account").with_icon_scale(ACCOUNT_ICON_SCALE),
    ]
}

fn tab_swipe_reference_page(progress: f32) -> usize {
    let progress = progress.clamp(0.0, 1.0);
    if progress < 0.5 {
        0
    } else if progress < 0.92 {
        2
    } else {
        3
    }
}

pub fn set_tab_swipe_reference_page(page: usize) -> Result<(), String> {
    if !matches!(page, 0 | 2 | 3) {
        return Err(format!("unsupported tab-swipe reference page {page}"));
    }
    let state = TAB_SWIPE_REFERENCE_PAGE_OVERRIDE
        .with(|cell| cell.borrow().as_ref().copied())
        .ok_or_else(|| "tab-swipe reference page state is not installed".to_string())?;
    state.set(Some(page));
    Ok(())
}

fn heading_style(color: Color) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            font_size: TextUnit::Sp(15.0),
            font_weight: Some(cranpose::text::FontWeight::SEMI_BOLD),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn body_style(color: Color) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            font_size: TextUnit::Sp(14.0),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[composable]
fn TogglePressReferenceStage(checked: bool, on_change: impl Fn(bool) + 'static) {
    let layout = toggle_press_reference_layout();
    let on_change = std::rc::Rc::new(on_change);
    Box(
        Modifier::empty()
            .required_size(layout.size)
            .draw_behind(move |scope| {
                scope.draw_rect(Brush::solid(layout.background));
                let card = Brush::solid(Color::WHITE);
                scope.draw_rect_at(
                    Rect {
                        x: 0.0,
                        y: layout.backdrop_cap_center.y - layout.backdrop_cap_radius,
                        width: layout.backdrop_cap_center.x,
                        height: layout.backdrop_cap_radius * 2.0,
                    },
                    card.clone(),
                );
                scope.draw_circle(card, layout.backdrop_cap_center, layout.backdrop_cap_radius);
            }),
        BoxSpec::default(),
        move || {
            let on_change = std::rc::Rc::clone(&on_change);
            LiquidToggle(
                Modifier::empty()
                    .offset(layout.track_origin.x, layout.track_origin.y)
                    .semantics(|config| {
                        config.role = Some(SemanticsWidgetRole::Button);
                        config.is_clickable = true;
                        config.content_description = Some("Wi-Fi switch".to_string());
                    }),
                checked,
                move |value| on_change(value),
            );
        },
    );
}

fn touched_up_action_spec() -> GlassButtonSpec {
    let action = Color::from_rgb_u8(0, 199, 208);
    GlassButtonSpec::prominent()
        .with_glass(
            Glass::regular()
                .tint(action.with_alpha(0.82))
                .blur_radius(3.0)
                .saturation(1.15)
                .lift(0.03)
                .highlight(0.72)
                .adaptive_frost(Color::WHITE, 0.12),
        )
        .with_content_color(Color::WHITE)
}

#[composable]
fn TouchedUpButtonGroup(
    confirmed: cranpose_core::MutableState<bool>,
    action_spec: GlassButtonSpec,
) {
    let more_confirmed = confirmed;
    let check_confirmed = confirmed;
    let mut items = vec![GlassIconButtonGroupItem::new(
        icons::MORE_HORIZ,
        "More grouped action",
        move || more_confirmed.set(false),
    )];
    if !confirmed.get() {
        items.push(
            GlassIconButtonGroupItem::new(icons::CHECK, "Confirm grouped action", move || {
                check_confirmed.set(true)
            })
            .with_spec(action_spec),
        );
    }
    GlassIconButtonGroup(
        Modifier::empty(),
        GlassIconButtonGroupSpec::new(44.0)
            .with_spacing(14.0)
            .with_glue_radius(36.0),
        items,
    );
}

#[composable]
fn TouchedUpReferenceStage() {
    let confirmed = remember(|| mutableStateOf(false)).with(|s| *s);
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(82.0)
            .padding_each(0.0, 0.0, 96.0, 0.0),
        BoxSpec::default().content_alignment(Alignment::new(
            HorizontalAlignment::End,
            VerticalAlignment::CenterVertically,
        )),
        move || TouchedUpButtonGroup(confirmed, touched_up_action_spec()),
    );
}

#[composable]
fn TouchedUpFixtureStage() {
    let confirmed = remember(|| mutableStateOf(false)).with(|s| *s);
    Box(
        Modifier::empty()
            .size(Size::new(220.0, 236.0))
            .draw_behind(|scope| {
                scope.draw_rect(Brush::solid(Color::from_rgb_u8(241, 240, 247)));
                scope.draw_circle(Brush::solid(Color::BLACK), Point::new(0.0, 48.0), 23.0);
                scope.draw_round_rect_at(
                    Rect {
                        x: 0.0,
                        y: 172.0,
                        width: 220.0,
                        height: 96.0,
                    },
                    Brush::solid(Color::WHITE),
                    CornerRadii::uniform(30.0),
                );
            }),
        BoxSpec::default(),
        move || {
            Text(
                "••••     Wi-Fi     64",
                Modifier::empty().offset(43.0, 18.0),
                TextStyle {
                    span_style: SpanStyle {
                        color: Some(Color::BLACK),
                        font_size: TextUnit::Sp(14.0),
                        font_weight: Some(cranpose::text::FontWeight::BOLD),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
            Box(
                Modifier::empty()
                    .fill_max_width()
                    .height(82.0)
                    .offset(0.0, 60.0)
                    .padding_each(0.0, 0.0, 18.0, 0.0),
                BoxSpec::default().content_alignment(Alignment::new(
                    HorizontalAlignment::End,
                    VerticalAlignment::CenterVertically,
                )),
                move || TouchedUpButtonGroup(confirmed, touched_up_action_spec()),
            );
        },
    );
}

fn clamp_optical_preview_center(position: Point, stage_size: Size) -> Point {
    let half_width = OPTICAL_PREVIEW_LENS_SIZE.width * 0.5;
    let half_height = OPTICAL_PREVIEW_LENS_SIZE.height * 0.5;
    Point::new(
        position
            .x
            .clamp(half_width, (stage_size.width - half_width).max(half_width)),
        position.y.clamp(
            half_height,
            (stage_size.height - half_height).max(half_height),
        ),
    )
}

fn initial_optical_preview_center(stage_size: Size) -> Point {
    clamp_optical_preview_center(
        Point::new(
            stage_size.width * 0.5,
            stage_size.height * OPTICAL_PREVIEW_INITIAL_Y_FRACTION,
        ),
        stage_size,
    )
}

fn optical_preview_glass(depth: f32, curve: f32, blur: f32, chroma: f32, light: f32) -> Glass {
    Glass::lens()
        .shape(LiquidShape::Capsule)
        .tint(Color::WHITE.with_alpha(0.04))
        .refraction_depth(depth.clamp(0.0, 1.0))
        .refraction_curve(curve.clamp(0.0, 1.0))
        .blur_radius(blur.clamp(0.0, 1.0) * 8.0)
        .dispersion(chroma.clamp(0.0, 1.0))
        .saturation(1.08)
        .highlight(light.clamp(0.0, 1.0))
        .no_clip()
}

#[composable]
fn OpticalParameterRow(label: &'static str, value: f32, on_change: impl Fn(f32) + 'static) {
    let colors = liquid_colors();
    let on_change = std::rc::Rc::new(on_change);
    Row(
        Modifier::empty()
            .fill_max_width()
            .height(32.0)
            .padding_symmetric(8.0, 0.0),
        RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            let slider_change = std::rc::Rc::clone(&on_change);
            Text(
                label,
                Modifier::empty().width(64.0),
                body_style(colors.label),
            );
            LiquidSlider(
                Modifier::empty().weight(1.0),
                value.clamp(0.0, 1.0),
                move |next| slider_change(next),
            );
            Text(
                format!("{:.2}", value.clamp(0.0, 1.0)),
                Modifier::empty()
                    .width(44.0)
                    .padding_each(8.0, 0.0, 0.0, 0.0),
                body_style(colors.secondary_label),
            );
        },
    );
}

#[composable]
fn OpticalShaderPreview() {
    let center = remember(|| mutableStateOf(Option::<Point>::None)).with(|state| *state);
    let depth = remember(|| mutableStateOf(0.34f32)).with(|state| *state);
    let curve = remember(|| mutableStateOf(0.25f32)).with(|state| *state);
    let blur = remember(|| mutableStateOf(0.0f32)).with(|state| *state);
    let chroma = remember(|| mutableStateOf(0.30f32)).with(|state| *state);
    let light = remember(|| mutableStateOf(0.34f32)).with(|state| *state);

    Column(
        Modifier::empty().fill_max_width(),
        ColumnSpec::default(),
        move || {
            let depth_state = depth;
            OpticalParameterRow("Depth", depth.get(), move |value| depth_state.set(value));
            let curve_state = curve;
            OpticalParameterRow("Curve", curve.get(), move |value| curve_state.set(value));
            let blur_state = blur;
            OpticalParameterRow("Blur", blur.get(), move |value| blur_state.set(value));
            let chroma_state = chroma;
            OpticalParameterRow("Chroma", chroma.get(), move |value| chroma_state.set(value));
            let light_state = light;
            OpticalParameterRow("Light", light.get(), move |value| light_state.set(value));
            Box(Modifier::empty().height(8.0), BoxSpec::default(), || {});

            BoxWithConstraints(
                Modifier::empty()
                    .fill_max_width()
                    .height(OPTICAL_PREVIEW_HEIGHT),
                move |scope| {
                    let stage_size = Size::new(
                        scope
                            .constraints()
                            .max_width
                            .max(OPTICAL_PREVIEW_LENS_SIZE.width),
                        OPTICAL_PREVIEW_HEIGHT,
                    );
                    let stage = Modifier::empty()
                        .fill_max_width()
                        .height(OPTICAL_PREVIEW_HEIGHT)
                        .pointer_input(
                            (stage_size.width.to_bits(), stage_size.height.to_bits()),
                            move |scope: PointerInputScope| async move {
                                scope
                                    .await_pointer_event_scope(|await_scope| async move {
                                        let mut dragging = false;
                                        loop {
                                            let event = await_scope.await_pointer_event().await;
                                            match event.kind {
                                                PointerEventKind::Down => {
                                                    dragging = true;
                                                    center.set(Some(clamp_optical_preview_center(
                                                        event.position,
                                                        stage_size,
                                                    )));
                                                    event.consume();
                                                }
                                                PointerEventKind::Move if dragging => {
                                                    center.set(Some(clamp_optical_preview_center(
                                                        event.position,
                                                        stage_size,
                                                    )));
                                                    event.consume();
                                                }
                                                PointerEventKind::Up | PointerEventKind::Cancel
                                                    if dragging =>
                                                {
                                                    dragging = false;
                                                    event.consume();
                                                }
                                                _ => {}
                                            }
                                        }
                                    })
                                    .await;
                            },
                        );

                    Box(stage, BoxSpec::default(), move || {
                        Row(
                            Modifier::empty().fill_max_size(),
                            RowSpec::default(),
                            move || {
                                BackdropTile(
                                    Modifier::empty().weight(1.0).fill_max_height(),
                                    "Utilities",
                                    Color::from_rgb_u8(112, 54, 14),
                                    Color::from_rgb_u8(255, 190, 111),
                                    Color::from_rgb_u8(225, 126, 41),
                                    CalculatorBackdropGraphic,
                                );
                                Box(Modifier::empty().width(10.0), BoxSpec::default(), || {});
                                BackdropTile(
                                    Modifier::empty().weight(1.0).fill_max_height(),
                                    "Social Networking",
                                    Color::from_rgb_u8(35, 22, 84),
                                    Color::from_rgb_u8(174, 157, 246),
                                    Color::from_rgb_u8(91, 62, 190),
                                    SpheresBackdropGraphic,
                                );
                            },
                        );

                        let lens_center = center
                            .get()
                            .unwrap_or_else(|| initial_optical_preview_center(stage_size));
                        GlassSurface(
                            Modifier::empty()
                                .required_size(OPTICAL_PREVIEW_LENS_SIZE)
                                .offset(
                                    lens_center.x - OPTICAL_PREVIEW_LENS_SIZE.width * 0.5,
                                    lens_center.y - OPTICAL_PREVIEW_LENS_SIZE.height * 0.5,
                                ),
                            optical_preview_glass(
                                depth.get(),
                                curve.get(),
                                blur.get(),
                                chroma.get(),
                                light.get(),
                            ),
                            || {},
                        );
                    });
                },
            );
        },
    );
}

/// A vivid gradient block — the kind of backdrop glass is made for.
#[composable]
fn GradientStripe(colors: [Color; 2], height: f32) {
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(height)
            .draw_behind(move |scope| {
                let size = scope.size();
                scope.draw_round_rect(
                    Brush::linear_gradient_range(
                        vec![colors[0], colors[1]],
                        Point::new(0.0, 0.0),
                        Point::new(size.width, size.height),
                    ),
                    CornerRadii::uniform(22.0),
                );
            }),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn TabSwipeBackdropContent(page: usize, offset_x: f32) {
    let layout = tab_swipe_reference_layout();
    let secondary = Color::from_rgb_u8(112, 112, 120);
    let label = Color::from_rgb_u8(24, 24, 28);
    let modifier = Modifier::empty().size(layout.size);
    let modifier = if offset_x == 0.0 {
        modifier
    } else {
        modifier.offset(offset_x, 0.0)
    };
    Box(
        modifier.size(layout.size).draw_behind(|scope| {
            scope.draw_rect(Brush::solid(Color::from_rgb_u8(245, 245, 251)));
        }),
        BoxSpec::default(),
        move || match page {
            0 | 1 => {
                Box(
                    Modifier::empty()
                        .size(Size::new(122.0, 66.0))
                        .offset(20.0, 6.0)
                        .draw_behind(|scope| {
                            scope.draw_round_rect(
                                Brush::linear_gradient_range(
                                    vec![
                                        Color::from_rgb_u8(75, 119, 180),
                                        Color::from_rgb_u8(232, 198, 149),
                                    ],
                                    Point::new(0.0, 0.0),
                                    Point::new(scope.size().width, scope.size().height),
                                ),
                                CornerRadii::uniform(8.0),
                            );
                        }),
                    BoxSpec::default(),
                    || {},
                );
                Text(
                    "WWDC26",
                    Modifier::empty().offset(154.0, 7.0),
                    TextStyle {
                        span_style: SpanStyle {
                            color: Some(secondary),
                            font_size: TextUnit::Sp(11.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                );
                Text(
                    "Build a responsive camera app that\nlaunches quickly",
                    Modifier::empty().offset(154.0, 23.0),
                    TextStyle {
                        span_style: SpanStyle {
                            color: Some(label),
                            font_size: TextUnit::Sp(14.0),
                            font_weight: Some(cranpose::text::FontWeight::SEMI_BOLD),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                );
            }
            2 => {
                Box(
                    Modifier::empty()
                        .size(Size::new(layout.xcode.width, layout.xcode.height))
                        .offset(layout.xcode.x, layout.xcode.y)
                        .draw_behind(|scope| {
                            scope.draw_round_rect(
                                Brush::solid(Color::from_rgb_u8(250, 250, 252)),
                                CornerRadii::uniform(18.0),
                            );
                        }),
                    BoxSpec::default(),
                    move || {
                        Text(
                            "Xcode",
                            Modifier::empty().offset(30.0, 8.0),
                            TextStyle {
                                span_style: SpanStyle {
                                    color: Some(Color::from_rgb_u8(208, 208, 214)),
                                    font_size: TextUnit::Sp(18.0),
                                    font_weight: Some(cranpose::text::FontWeight::BOLD),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                    },
                );
                GlassSurface(
                    Modifier::empty()
                        .size(Size::new(layout.enroll.width, layout.enroll.height))
                        .offset(layout.enroll.x, layout.enroll.y),
                    tab_swipe_reference_glass(),
                    || {
                        Box(
                            Modifier::empty().fill_max_size(),
                            BoxSpec::default(),
                            || {
                                Text(
                                    "Enroll Now",
                                    Modifier::empty().offset(20.0, 12.0),
                                    TextStyle {
                                        span_style: SpanStyle {
                                            color: Some(Color::from_rgb_u8(0, 122, 255)),
                                            font_size: TextUnit::Sp(17.0),
                                            ..Default::default()
                                        },
                                        ..Default::default()
                                    },
                                );
                            },
                        );
                    },
                );
                Text(
                    "Apple Developer Program",
                    Modifier::empty().offset(layout.title.x, layout.title.y),
                    TextStyle {
                        span_style: SpanStyle {
                            color: Some(Color::from_rgb_u8(124, 124, 130)),
                            font_size: TextUnit::Sp(17.0),
                            font_weight: Some(cranpose::text::FontWeight::MEDIUM),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                );
            }
            _ => {
                Text(
                    "Explore the latest in watchOS.",
                    Modifier::empty().offset(36.0, 8.0),
                    body_style(secondary),
                );
                Box(
                    Modifier::empty()
                        .size(Size::new(376.0, 52.0))
                        .offset(32.0, 32.0)
                        .draw_behind(|scope| {
                            scope.draw_round_rect(
                                Brush::solid(Color::from_rgb_u8(250, 250, 252)),
                                CornerRadii::uniform(14.0),
                            );
                        }),
                    BoxSpec::default(),
                    move || {
                        Text(
                            "Xcode",
                            Modifier::empty().offset(28.0, 13.0),
                            heading_style(label),
                        );
                    },
                );
            }
        },
    );
}

#[composable]
fn TabSwipeReferenceBackdrop() {
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(tab_swipe_reference_layout().size.height),
        BoxSpec::default().content_alignment(Alignment::new(
            HorizontalAlignment::CenterHorizontally,
            VerticalAlignment::Top,
        )),
        || TabSwipeBackdropContent(2, 4.0),
    );
}

#[composable]
fn TabSwipeFixtureBackdrop(page: usize) {
    TabSwipeBackdropContent(page, 0.0);
}

#[composable]
fn BackdropTile(
    modifier: Modifier,
    label: &'static str,
    label_color: Color,
    start: Color,
    end: Color,
    content: impl FnMut() + 'static,
) {
    Box(
        modifier.draw_behind(move |scope| {
            scope.draw_round_rect(
                Brush::linear_gradient_range(
                    vec![start, end],
                    Point::new(0.0, 0.0),
                    Point::new(0.0, scope.size().height),
                ),
                CornerRadii::uniform(16.0),
            );
        }),
        BoxSpec::default(),
        move || {
            Text(
                label,
                Modifier::empty().offset(12.0, 76.0),
                TextStyle {
                    span_style: SpanStyle {
                        color: Some(label_color),
                        font_size: TextUnit::Sp(24.0),
                        font_weight: Some(cranpose::text::FontWeight::BOLD),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
            content();
        },
    );
}

#[composable]
fn CalculatorBackdropGraphic() {
    Box(
        Modifier::empty()
            .size(Size::new(52.0, 64.0))
            .offset(108.0, 3.0)
            .graphics_layer(|| GraphicsLayer {
                rotation_z: 5.0,
                ..Default::default()
            })
            .drop_shadow(
                cranpose_ui_graphics::LayerShape::Rounded(
                    cranpose_ui_graphics::RoundedCornerShape::uniform(7.0),
                ),
                |scope| {
                    scope.radius = 8.0;
                    scope.offset.y = 4.0;
                    scope.color = Color::BLACK.with_alpha(0.18);
                },
            )
            .draw_behind(|scope| {
                scope.draw_round_rect(
                    Brush::linear_gradient_range(
                        vec![
                            Color::from_rgb_u8(226, 228, 220),
                            Color::from_rgb_u8(173, 178, 171),
                        ],
                        Point::new(0.0, 0.0),
                        Point::new(scope.size().width, scope.size().height),
                    ),
                    CornerRadii::uniform(7.0),
                );
                scope.draw_rect_at(
                    Rect {
                        x: 6.0,
                        y: 7.0,
                        width: 40.0,
                        height: 15.0,
                    },
                    Brush::solid(Color::from_rgb_u8(86, 103, 73)),
                );
                for row in 0..3 {
                    for column in 0..4 {
                        scope.draw_circle(
                            Brush::solid(if column == 3 {
                                Color::from_rgb_u8(247, 191, 48)
                            } else {
                                Color::from_rgb_u8(112, 116, 111)
                            }),
                            Point::new(10.0 + column as f32 * 10.5, 31.0 + row as f32 * 10.5),
                            3.5,
                        );
                    }
                }
            }),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn SpheresBackdropGraphic() {
    Box(
        Modifier::empty()
            .size(Size::new(92.0, 67.0))
            .offset(92.0, 0.0)
            .draw_behind(|scope| {
                scope.draw_circle(
                    Brush::linear_gradient_range(
                        vec![
                            Color::from_rgb_u8(255, 78, 105),
                            Color::from_rgb_u8(196, 35, 70),
                        ],
                        Point::new(35.0, 2.0),
                        Point::new(50.0, 54.0),
                    ),
                    Point::new(43.0, 28.0),
                    27.0,
                );
                scope.draw_circle(
                    Brush::linear_gradient_range(
                        vec![
                            Color::from_rgb_u8(73, 220, 239),
                            Color::from_rgb_u8(38, 107, 188),
                        ],
                        Point::new(48.0, 18.0),
                        Point::new(76.0, 62.0),
                    ),
                    Point::new(65.0, 43.0),
                    23.0,
                );
                scope.draw_circle(
                    Brush::linear_gradient_range(
                        vec![
                            Color::from_rgb_u8(244, 173, 255),
                            Color::from_rgb_u8(113, 61, 210),
                        ],
                        Point::new(12.0, 32.0),
                        Point::new(42.0, 66.0),
                    ),
                    Point::new(29.0, 49.0),
                    18.0,
                );
            }),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn BottomBarBackdrop() {
    Row(
        Modifier::empty()
            .width(440.0)
            .height(112.0)
            .padding_symmetric(20.0, 0.0),
        RowSpec::default(),
        || {
            BackdropTile(
                Modifier::empty().weight(1.0).fill_max_height(),
                "Outrageous",
                Color::from_rgba_u8(88, 42, 12, 220),
                Color::from_rgb_u8(255, 190, 119),
                Color::from_rgb_u8(221, 119, 31),
                CalculatorBackdropGraphic,
            );
            Box(Modifier::empty().width(10.0), BoxSpec::default(), || {});
            BackdropTile(
                Modifier::empty().weight(1.0).fill_max_height(),
                "Custom Networking",
                Color::from_rgba_u8(255, 255, 255, 220),
                Color::from_rgb_u8(173, 154, 248),
                Color::from_rgb_u8(86, 65, 182),
                SpheresBackdropGraphic,
            );
        },
    );
}

#[composable]
fn StoreBottomBarExample(selected: usize, on_select: impl Fn(usize) + 'static) {
    let on_select: std::rc::Rc<dyn Fn(usize)> = std::rc::Rc::new(on_select);
    Box(
        Modifier::empty().width(440.0).height(112.0),
        BoxSpec::default().content_alignment(Alignment::new(
            HorizontalAlignment::CenterHorizontally,
            VerticalAlignment::Bottom,
        )),
        {
            let on_select = std::rc::Rc::clone(&on_select);
            move || {
                BottomBarBackdrop();
                Box(
                    Modifier::empty()
                        .fill_max_size()
                        .padding_each(0.0, 0.0, 0.0, 10.0),
                    BoxSpec::default().content_alignment(Alignment::new(
                        HorizontalAlignment::CenterHorizontally,
                        VerticalAlignment::Bottom,
                    )),
                    {
                        let on_select = std::rc::Rc::clone(&on_select);
                        move || {
                            let on_select = std::rc::Rc::clone(&on_select);
                            LiquidTabBar(
                                Modifier::empty(),
                                LiquidTabBarSpec::default(),
                                vec![
                                    LiquidTab::new(icons::DOCUMENT, "Today"),
                                    LiquidTab::new(icons::ROCKET, "Games"),
                                    LiquidTab::new(icons::LAYERS, "Apps"),
                                    LiquidTab::new(icons::JOYSTICK, "Arcade"),
                                    LiquidTab::new(icons::SEARCH, "Search"),
                                ],
                                selected,
                                move |index| on_select(index),
                            );
                        }
                    },
                );
            }
        },
    );
}

/// The menu-expand reference composition: a deep-purple header hosting a
/// bright magenta filter pill; tapping it opens a DARK sort/filter menu
/// whose "Sort by" / "Filter" rows expand the container in place (the
/// accordion morph from the 16-jul recording).
#[composable]
fn SortFilterStage(suggestion_offset: f32) {
    let menu_open = remember(|| mutableStateOf(false)).with(|s| *s);
    // 0 = collapsed, 1 = "Sort by" expanded, 2 = "Filter" expanded.
    let section = remember(|| mutableStateOf(0usize)).with(|s| *s);
    let sort_choice = remember(|| mutableStateOf(0usize)).with(|s| *s);
    let filter_choice = remember(|| mutableStateOf(0usize)).with(|s| *s);
    let anchor_rect = remember(|| {
        std::rc::Rc::new(std::cell::Cell::new(Rect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        }))
    })
    .with(std::rc::Rc::clone);
    let menu_gesture = remember_liquid_menu_gesture();

    LiquidTheme(
        LiquidThemeSpec {
            scheme: SchemeMode::Dark,
            accent: Color::from_rgb_u8(233, 30, 196),
            ..LiquidThemeSpec::default()
        },
        move || {
            let anchor_rect = std::rc::Rc::clone(&anchor_rect);
            let menu_gesture = menu_gesture.clone();
            Box(
                Modifier::empty().width(440.0).height(300.0),
                BoxSpec::default(),
                move || {
                    let anchor_sink = std::rc::Rc::clone(&anchor_rect);
                    let column_gesture = menu_gesture.clone();
                    Column(Modifier::empty().fill_max_size(), ColumnSpec::default(), {
                        move || {
                            let column_gesture = column_gesture.clone();
                            // Header band: deep purple with the magenta pill.
                            let pill_open = menu_open;
                            Box(
                                // Reference proportions (menu-expand f_001):
                                // the header spans ~29% of the screen and the
                                // opened panel's top third overlaps it.
                                Modifier::empty()
                                    .fill_max_width()
                                    .height(120.0)
                                    .draw_behind(|scope| {
                                        // Measured on menu-expand f_001: the
                                        // header is nearly FLAT deep purple
                                        // (53,13,54)..(58,17,58) — the old
                                        // (30,4,28) top starved the panel's
                                        // bloom curve of ~30% of its light.
                                        scope.draw_rect(Brush::linear_gradient_range(
                                            vec![
                                                Color::from_rgb_u8(53, 13, 54),
                                                Color::from_rgb_u8(58, 17, 58),
                                            ],
                                            Point::new(0.0, 0.0),
                                            Point::new(0.0, scope.size().height),
                                        ));
                                    }),
                                BoxSpec::default().content_alignment(Alignment::new(
                                    HorizontalAlignment::End,
                                    VerticalAlignment::CenterVertically,
                                )),
                                {
                                    let anchor_sink = std::rc::Rc::clone(&anchor_sink);
                                    let pill_gesture = column_gesture.clone();
                                    move || {
                                        let pill_open = pill_open;
                                        // Touched glass rises toward the user
                                        // (reference lift) on a spring, and
                                        // the same coordinate drives the HDR
                                        // glow so light and geometry move as
                                        // one.
                                        let pill_press = cranpose_animation::animateFloatAsState(
                                            if pill_gesture.is_pressed() { 1.0 } else { 0.0 },
                                            cranpose_animation::spring(1.0, 600.0),
                                            "sortfilter-pill-press",
                                        );
                                        let pill = liquid_menu_trigger_input(
                                            Modifier::empty(),
                                            pill_gesture.clone(),
                                            move || pill_open.set(true),
                                        )
                                        .graphics_layer(move || {
                                            let press = pill_press.get().clamp(0.0, 1.0);
                                            GraphicsLayer {
                                                scale_x: 1.0 + 0.05 * press,
                                                scale_y: 1.0 + 0.05 * press,
                                                translation_y: -1.5 * press,
                                                ..Default::default()
                                            }
                                        })
                                        .size(Size::new(96.0, 40.0))
                                        .offset(-16.0, 0.0)
                                        .report_window_rect(std::rc::Rc::clone(&anchor_sink))
                                        .semantics(|config| {
                                            config.role = Some(SemanticsWidgetRole::Button);
                                            config.is_clickable = true;
                                            config.content_description =
                                                Some("Sort filter pill".to_string());
                                        })
                                        .draw_behind(|scope| {
                                            scope.draw_round_rect(
                                                Brush::solid(Color::from_rgb_u8(233, 30, 196)),
                                                CornerRadii::uniform(20.0),
                                            );
                                        })
                                        // Touched glass answers with its
                                        // material: HDR-like highlight +
                                        // saturation and the under-finger
                                        // glow while the gesture is down.
                                        .glass_effect_with(
                                            Glass::clear()
                                                .shape(LiquidShape::Capsule)
                                                .blur_radius(0.0)
                                                .shadow(false)
                                                .highlight(0.0)
                                                .no_clip(),
                                            {
                                                let glow_gesture = pill_gesture.clone();
                                                let glow_rect = std::rc::Rc::clone(&anchor_sink);
                                                move || {
                                                    let press = if glow_gesture.is_pressed() {
                                                        1.0
                                                    } else {
                                                        0.0
                                                    };
                                                    let touch =
                                                        glow_gesture.press_point().map(|point| {
                                                            let rect = glow_rect.get();
                                                            (
                                                                point.x - rect.x,
                                                                point.y - rect.y,
                                                                1.0f32,
                                                            )
                                                        });
                                                    let rect = glow_rect.get();
                                                    GlassDynamics {
                                                        activity: Some(press),
                                                        touch: None,
                                                        ..Default::default()
                                                    }
                                                    .touched_up(
                                                        press,
                                                        touch.map(|(x, y, _)| (x, y)),
                                                        (rect.width * 0.5, rect.height * 0.5),
                                                    )
                                                }
                                            },
                                        );
                                        let pill_press_fill = pill_press;
                                        Box(
                                            pill,
                                            BoxSpec::default().content_alignment(Alignment::CENTER),
                                            move || {
                                                // Reference pill (menu-expand
                                                // f_001): a dark capsule ringed
                                                // by the magenta accent, not a
                                                // solid magenta slab. The PRESS
                                                // floods the face with that
                                                // accent (open strip 200-366ms:
                                                // the whole capsule blazes hot
                                                // magenta while held) — the dark
                                                // inner yields to the ring's
                                                // light instead of painting a
                                                // new color.
                                                Box(
                                                    Modifier::empty()
                                                        .fill_max_size()
                                                        .padding(1.5)
                                                        .draw_behind(move |scope| {
                                                            let press = pill_press_fill
                                                                .get()
                                                                .clamp(0.0, 1.0);
                                                            scope.draw_round_rect(
                                                                Brush::solid(
                                                                    Color::from_rgb_u8(58, 14, 54)
                                                                        .with_alpha(
                                                                            1.0 - 0.92 * press,
                                                                        ),
                                                                ),
                                                                CornerRadii::uniform(18.5),
                                                            );
                                                        }),
                                                    BoxSpec::default()
                                                        .content_alignment(Alignment::CENTER),
                                                    || {
                                                        Row(
                                                            Modifier::empty(),
                                                            RowSpec::default(),
                                                            || {
                                                                icons::Icon(
                                                                    icons::FILTER,
                                                                    20.0,
                                                                    Color::WHITE,
                                                                );
                                                                Box(
                                                                    Modifier::empty().width(14.0),
                                                                    BoxSpec::default(),
                                                                    || {},
                                                                );
                                                                icons::Icon(
                                                                    icons::ACCOUNT_CIRCLE,
                                                                    28.0,
                                                                    Color::WHITE,
                                                                );
                                                            },
                                                        );
                                                    },
                                                );
                                            },
                                        );
                                    }
                                },
                            );
                            // Light page under the header: the two suggestion
                            // tiles from the recording.
                            Box(
                                Modifier::empty()
                                    .fill_max_width()
                                    .height(180.0)
                                    .draw_behind(|scope| {
                                        scope.draw_rect(Brush::solid(Color::from_rgb_u8(
                                            250, 250, 252,
                                        )));
                                    }),
                                BoxSpec::default(),
                                move || {
                                    let row_modifier = Modifier::empty();
                                    let row_modifier = if suggestion_offset == 0.0 {
                                        row_modifier
                                    } else {
                                        row_modifier.offset(suggestion_offset, 0.0)
                                    };
                                    Row(
                                        row_modifier.padding_each(16.0, 18.0, 16.0, 0.0),
                                        RowSpec::default(),
                                        || {
                                            SuggestionTile("Later", "1 item");
                                            Box(
                                                Modifier::empty().width(12.0),
                                                BoxSpec::default(),
                                                || {},
                                            );
                                            SuggestionTile("Drafts & sent", "0 items");
                                        },
                                    );
                                },
                            );
                        }
                    });

                    // The dark accordion menu out of the pill.
                    let items = sort_filter_menu_items(
                        section.get(),
                        sort_choice.get(),
                        filter_choice.get(),
                    );
                    let labels: Vec<String> = items.iter().map(|item| item.label.clone()).collect();
                    let dismiss_open = menu_open;
                    let dismiss_section = section;
                    LiquidMenu(
                        menu_open.get(),
                        anchor_rect.get(),
                        LiquidMenuSpec::new(290.0),
                        Vec::new(),
                        items,
                        menu_gesture.clone(),
                        move |index| match labels.get(index).map(String::as_str) {
                            Some("Sort by") => section.set(if section.get() == 1 { 0 } else { 1 }),
                            Some("Filter") => section.set(if section.get() == 2 { 0 } else { 2 }),
                            Some("Sections") => sort_choice.set(0),
                            Some("Newest to oldest") => sort_choice.set(1),
                            Some("All conversations") => filter_choice.set(0),
                            Some("External people") => filter_choice.set(1),
                            _ => {}
                        },
                        move || {
                            dismiss_open.set(false);
                            dismiss_section.set(0);
                        },
                    );
                },
            );
        },
    );
}

#[composable]
fn SortFilterReferenceStage() {
    SortFilterStage(0.0);
}

#[composable]
fn SortFilterFixtureStage() {
    SortFilterStage(104.0);
}

/// One white suggestion tile ("Later / 1 item") from the recording's page.
#[composable]
fn SuggestionTile(title: &'static str, subtitle: &'static str) {
    Box(
        Modifier::empty()
            .width(120.0)
            .height(64.0)
            .draw_behind(|scope| {
                scope.draw_round_rect(Brush::solid(Color::WHITE), CornerRadii::uniform(12.0));
            }),
        BoxSpec::default(),
        move || {
            Column(
                Modifier::empty().padding_each(12.0, 10.0, 8.0, 0.0),
                ColumnSpec::default(),
                move || {
                    Text(
                        title,
                        Modifier::empty(),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(Color::from_rgb_u8(30, 30, 36)),
                                font_size: TextUnit::Sp(13.0),
                                font_weight: Some(cranpose::text::FontWeight::MEDIUM),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    );
                    Text(
                        subtitle,
                        Modifier::empty(),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(Color::from_rgb_u8(120, 120, 128)),
                                font_size: TextUnit::Sp(12.0),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    );
                },
            );
        },
    );
}

/// The item list for the sort/filter accordion at a given expansion state.
fn sort_filter_menu_items(
    section: usize,
    sort_choice: usize,
    filter_choice: usize,
) -> Vec<LiquidMenuItem> {
    let mut items = vec![LiquidMenuItem::new("Sort by")
        .keeps_open()
        .subtitle("Sections. Unread messages on top.")];
    if section == 1 {
        items.push(
            LiquidMenuItem::new("Sections")
                .icon(icons::LIST_OUTLINE)
                .checked(sort_choice == 0),
        );
        items.push(
            LiquidMenuItem::new("Newest to oldest")
                .icon(icons::SCHEDULE)
                .checked(sort_choice == 1),
        );
    }
    items.push(
        LiquidMenuItem::new("Filter")
            .keeps_open()
            .section_start()
            .subtitle("All conversations"),
    );
    if section == 2 {
        items.push(LiquidMenuItem::new("All conversations").checked(filter_choice == 0));
        items.push(LiquidMenuItem::new("External people").checked(filter_choice == 1));
    }
    items
}

/// The reference `bar_headers_folded` composition: colored section headers
/// whose bold white titles cross the floating bar's top edge, so the bar's
/// fold renders them mirrored inside the glass.
#[composable]
fn StoreHeadersBarExample(selected: usize, on_select: impl Fn(usize) + 'static) {
    let on_select: std::rc::Rc<dyn Fn(usize)> = std::rc::Rc::new(on_select);
    Box(
        Modifier::empty()
            .width(440.0)
            .height(112.0)
            .semantics(|config| {
                config.role = Some(SemanticsWidgetRole::Button);
                config.is_clickable = true;
                config.content_description = Some("Headers fold stage".to_string());
            }),
        BoxSpec::default().content_alignment(Alignment::new(
            HorizontalAlignment::CenterHorizontally,
            VerticalAlignment::Bottom,
        )),
        {
            let on_select = std::rc::Rc::clone(&on_select);
            move || {
                Row(
                    Modifier::empty()
                        .width(440.0)
                        .height(112.0)
                        .padding_symmetric(20.0, 0.0),
                    RowSpec::default(),
                    || {
                        HeaderBlock(
                            Modifier::empty().weight(1.0).fill_max_height(),
                            "Puzzle Games",
                            Color::from_rgb_u8(63, 169, 214),
                            Color::from_rgb_u8(38, 128, 178),
                        );
                        Box(Modifier::empty().width(10.0), BoxSpec::default(), || {});
                        HeaderBlock(
                            Modifier::empty().weight(1.0).fill_max_height(),
                            "Strategy Games",
                            Color::from_rgb_u8(104, 189, 91),
                            Color::from_rgb_u8(58, 141, 66),
                        );
                    },
                );
                Box(
                    Modifier::empty()
                        .fill_max_size()
                        .padding_each(0.0, 0.0, 0.0, 10.0),
                    BoxSpec::default().content_alignment(Alignment::new(
                        HorizontalAlignment::CenterHorizontally,
                        VerticalAlignment::Bottom,
                    )),
                    {
                        let on_select = std::rc::Rc::clone(&on_select);
                        move || {
                            let on_select = std::rc::Rc::clone(&on_select);
                            LiquidTabBar(
                                Modifier::empty(),
                                LiquidTabBarSpec::default(),
                                vec![
                                    LiquidTab::new(icons::DOCUMENT, "Today"),
                                    LiquidTab::new(icons::ROCKET, "Games"),
                                    LiquidTab::new(icons::LAYERS, "Apps"),
                                    LiquidTab::new(icons::JOYSTICK, "Arcade"),
                                    LiquidTab::new(icons::SEARCH, "Search"),
                                ],
                                selected,
                                move |index| on_select(index),
                            );
                        }
                    },
                );
            }
        },
    );
}

/// One colored section-header block; the title sits low enough to cross the
/// floating bar's top edge (the fold's subject).
#[composable]
fn HeaderBlock(modifier: Modifier, title: &'static str, start: Color, end: Color) {
    Box(
        modifier.draw_behind(move |scope| {
            scope.draw_round_rect(
                Brush::linear_gradient_range(
                    vec![start, end],
                    Point::new(0.0, 0.0),
                    Point::new(0.0, scope.size().height),
                ),
                CornerRadii::uniform(16.0),
            );
        }),
        BoxSpec::default(),
        move || {
            Text(
                title,
                Modifier::empty().offset(14.0, 26.0),
                TextStyle {
                    span_style: SpanStyle {
                        color: Some(Color::WHITE),
                        font_size: TextUnit::Sp(19.0),
                        font_weight: Some(cranpose::text::FontWeight::BOLD),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
        },
    );
}

#[composable]
fn OnWhiteBottomBarExample(selected: usize, on_select: impl Fn(usize) + 'static) {
    let on_select: std::rc::Rc<dyn Fn(usize)> = std::rc::Rc::new(on_select);
    Box(
        Modifier::empty()
            .width(330.0)
            .height(98.0)
            .draw_behind(|scope| {
                scope.draw_rect(Brush::solid(Color::from_rgb_u8(241, 240, 247)));
            }),
        BoxSpec::default().content_alignment(Alignment::new(
            HorizontalAlignment::CenterHorizontally,
            VerticalAlignment::Bottom,
        )),
        move || {
            let on_select = std::rc::Rc::clone(&on_select);
            LiquidTheme(
                LiquidThemeSpec {
                    scheme: SchemeMode::Light,
                    accent: Color::from_rgb_u8(0, 190, 200),
                    ..LiquidThemeSpec::default()
                },
                move || {
                    let on_select = std::rc::Rc::clone(&on_select);
                    LiquidTabBar(
                        Modifier::empty().offset(5.0, -10.0),
                        // Reference on-white recording: cell pitch 319px over a
                        // 185px bar = 1.72x, i.e. 110dp cells against the 64dp
                        // bar. Narrower cells squash the held oval circular.
                        LiquidTabBarSpec::new(110.0),
                        vec![
                            LiquidTab::new(icons::TRANSLATE, "Translate"),
                            LiquidTab::new(icons::CAMERA, "Camera"),
                            LiquidTab::new(icons::GROUP, "Conversation"),
                        ],
                        selected,
                        move |index| on_select(index),
                    );
                },
            );
        },
    );
}

#[composable]
fn SectionTitle(text: &'static str) {
    let colors = liquid_colors();
    Text(
        text,
        Modifier::empty().padding_each(2.0, 18.0, 0.0, 8.0),
        heading_style(colors.secondary_label),
    );
}

/// One WWDC-style session card: dark glyph thumbnail, bold title, gray
/// subtitle — the list the reference floating tab bar hovers over.
#[composable]
fn SessionCard(icon: &'static str, title: &'static str, subtitle: &'static str) {
    LiquidCard(Modifier::empty().fill_max_width(), move || {
        Row(
            Modifier::empty().fill_max_width().padding(14.0),
            RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
            move || {
                let colors = liquid_colors();
                Box(
                    Modifier::empty()
                        .size(cranpose::Size::new(68.0, 68.0))
                        .draw_behind(move |scope| {
                            let size = scope.size();
                            scope.draw_round_rect(
                                Brush::linear_gradient_range(
                                    vec![
                                        Color::from_rgb_u8(44, 44, 52),
                                        Color::from_rgb_u8(8, 8, 12),
                                    ],
                                    Point::new(0.0, 0.0),
                                    Point::new(size.width, size.height),
                                ),
                                CornerRadii::uniform(18.0),
                            );
                        }),
                    BoxSpec::default().content_alignment(Alignment::CENTER),
                    move || {
                        icons::Icon(icon, 34.0, Color::from_rgb_u8(214, 222, 240));
                    },
                );
                Box(Modifier::empty().width(16.0), BoxSpec::default(), || {});
                Column(Modifier::empty().weight(1.0), ColumnSpec::default(), {
                    move || {
                        Text(
                            title,
                            Modifier::empty(),
                            TextStyle {
                                span_style: SpanStyle {
                                    color: Some(colors.label),
                                    font_size: TextUnit::Sp(21.0),
                                    font_weight: Some(cranpose::text::FontWeight::BOLD),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                        Box(Modifier::empty().height(3.0), BoxSpec::default(), || {});
                        Text(
                            subtitle,
                            Modifier::empty(),
                            body_style(colors.secondary_label),
                        );
                    }
                });
            },
        );
    });
    Box(Modifier::empty().height(12.0), BoxSpec::default(), || {});
}

/// The `menu-open` reference page block: the "Featured videos" header the
/// iPhone menu droplet grows over, with the WWDC26 session rows beneath —
/// the nav's filter/"..." circles sit directly above it at rest scroll, so
/// the composed frame matches `example/target/menu-open`.
#[composable]
fn FeaturedVideosReferenceStage(
    menu_open: cranpose_core::MutableState<bool>,
    menu_gesture: LiquidMenuGesture,
    anchor_sink: std::rc::Rc<std::cell::Cell<Rect>>,
    absorbed_sink: std::rc::Rc<std::cell::Cell<Rect>>,
    absorbed_button_spec: GlassButtonSpec,
) {
    Box(
        Modifier::empty().fill_max_width(),
        BoxSpec::default().content_alignment(Alignment::new(
            HorizontalAlignment::End,
            VerticalAlignment::Top,
        )),
        move || {
            FeaturedVideosReferenceCard(
                menu_open,
                menu_gesture.clone(),
                std::rc::Rc::clone(&anchor_sink),
                std::rc::Rc::clone(&absorbed_sink),
                absorbed_button_spec.clone(),
            );
        },
    );
    Box(Modifier::empty().height(12.0), BoxSpec::default(), || {});
}

#[composable]
fn FeaturedVideosReferenceCard(
    menu_open: cranpose_core::MutableState<bool>,
    menu_gesture: LiquidMenuGesture,
    anchor_sink: std::rc::Rc<std::cell::Cell<Rect>>,
    absorbed_sink: std::rc::Rc<std::cell::Cell<Rect>>,
    absorbed_button_spec: GlassButtonSpec,
) {
    let colors = liquid_colors();
    Column(
        Modifier::empty()
            .required_size(Size::new(330.0, 226.0))
            .semantics(|config| {
                config.role = Some(SemanticsWidgetRole::Button);
                config.is_clickable = true;
                config.content_description = Some("Featured videos".to_string());
            })
            .draw_behind(|scope| {
                scope.draw_round_rect(Brush::solid(Color::WHITE), CornerRadii::uniform(14.0));
            })
            .padding(14.0),
        ColumnSpec::default(),
        move || {
            // Reference header row: the title column with the filter and
            // "..." circles inline at its right — the droplet grows from
            // THIS anchor over the session rows below.
            let row_menu = menu_open;
            let row_gesture = menu_gesture.clone();
            let row_anchor = std::rc::Rc::clone(&anchor_sink);
            let row_absorbed = std::rc::Rc::clone(&absorbed_sink);
            let row_spec = absorbed_button_spec.clone();
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
                move || {
                    Column(Modifier::empty().weight(1.0), ColumnSpec::default(), {
                        move || {
                            Text(
                                "Featured videos",
                                Modifier::empty(),
                                TextStyle {
                                    span_style: SpanStyle {
                                        color: Some(colors.label),
                                        font_size: TextUnit::Sp(17.0),
                                        font_weight: Some(cranpose::text::FontWeight::BOLD),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                },
                            );
                            Text(
                                "1.328 of 1.328 Sessions",
                                Modifier::empty(),
                                body_style(colors.secondary_label),
                            );
                        }
                    });
                    LiquidMenuAbsorbedIconButton(
                        Modifier::empty().report_window_rect(std::rc::Rc::clone(&row_absorbed)),
                        row_spec.clone(),
                        36.0,
                        row_menu.get(),
                        || {},
                        icons::FILTER,
                    );
                    Box(Modifier::empty().width(6.0), BoxSpec::default(), || {});
                    LiquidMenuIconButton(
                        Modifier::empty().report_window_rect(std::rc::Rc::clone(&row_anchor)),
                        GlassButtonSpec::glass(),
                        40.0,
                        row_menu.get(),
                        row_gesture.clone(),
                        move || row_menu.set(true),
                        icons::MORE_HORIZ,
                    );
                },
            );
            Box(Modifier::empty().height(10.0), BoxSpec::default(), || {});
            for (index, day) in ["Dub Dub Daily: Day 5", "Dub Dub Daily: Day 4"]
                .into_iter()
                .enumerate()
            {
                if index > 0 {
                    Box(
                        Modifier::empty()
                            .fill_max_width()
                            .height(1.0)
                            .draw_behind(|scope| {
                                scope.draw_rect(Brush::solid(Color::from_rgb_u8(232, 232, 236)));
                            }),
                        BoxSpec::default(),
                        || {},
                    );
                    Box(Modifier::empty().height(10.0), BoxSpec::default(), || {});
                }
                Text(
                    "WWDC26",
                    Modifier::empty(),
                    TextStyle {
                        span_style: SpanStyle {
                            color: Some(colors.secondary_label),
                            font_size: TextUnit::Sp(12.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                );
                Box(Modifier::empty().height(2.0), BoxSpec::default(), || {});
                Row(
                    Modifier::empty().fill_max_width(),
                    RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
                    move || {
                        Text(
                            day,
                            Modifier::empty().weight(1.0),
                            TextStyle {
                                span_style: SpanStyle {
                                    color: Some(colors.label),
                                    font_size: TextUnit::Sp(16.0),
                                    font_weight: Some(cranpose::text::FontWeight::BOLD),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                        icons::Icon(icons::CHEVRON_RIGHT, 16.0, colors.secondary_label);
                    },
                );
                Box(Modifier::empty().height(10.0), BoxSpec::default(), || {});
            }
        },
    );
    Box(Modifier::empty().height(12.0), BoxSpec::default(), || {});
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiquidReferenceFixtureCase {
    TogglePress,
    MenuOpen,
    TabSwipe,
    Segmented,
    MenuExpand,
    BottomBarForm,
    OnWhiteClick,
    OnWhiteClickHold,
    OnWhiteTouchedUp,
}

#[composable]
fn MenuOpenReferenceStage() {
    let menu_open = remember(|| mutableStateOf(false)).with(|state| *state);
    let menu_selection = remember(|| mutableStateOf(0usize)).with(|state| *state);
    let menu_gesture = remember_liquid_menu_gesture();
    let menu_anchor_rect = remember(|| {
        std::rc::Rc::new(std::cell::Cell::new(Rect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        }))
    })
    .with(std::rc::Rc::clone);
    let menu_absorbed_rect = remember(|| {
        std::rc::Rc::new(std::cell::Cell::new(Rect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        }))
    })
    .with(std::rc::Rc::clone);
    let absorbed_button_spec = GlassButtonSpec::glass();

    Box(
        Modifier::empty().width(330.0).height(226.0),
        BoxSpec::default(),
        move || {
            let stage_gesture = menu_gesture.clone();
            FeaturedVideosReferenceStage(
                menu_open,
                stage_gesture,
                std::rc::Rc::clone(&menu_anchor_rect),
                std::rc::Rc::clone(&menu_absorbed_rect),
                absorbed_button_spec.clone(),
            );

            let menu_state = menu_open;
            let menu_dismiss = menu_open;
            let selection = menu_selection;
            LiquidMenu(
                menu_state.get(),
                menu_anchor_rect.get(),
                LiquidMenuSpec::default(),
                vec![
                    LiquidMenuAbsorbedSource::new(
                        menu_absorbed_rect.get(),
                        absorbed_button_spec.clone(),
                        36.0,
                        icons::FILTER,
                    ),
                    LiquidMenuAbsorbedSource::new(
                        menu_anchor_rect.get(),
                        GlassButtonSpec::glass(),
                        40.0,
                        icons::MORE_HORIZ,
                    ),
                ],
                vec![
                    LiquidMenuItem::new("List")
                        .icon(icons::LIST_OUTLINE)
                        .checked(selection.get() == 0),
                    LiquidMenuItem::new("Grid")
                        .icon(icons::GRID_OUTLINE)
                        .checked(selection.get() == 1),
                ],
                menu_gesture.clone(),
                move |index| selection.set(index),
                move || menu_dismiss.set(false),
            );
        },
    );
}

#[composable]
fn TabSwipeReferenceStage() {
    let selected = remember(|| mutableStateOf(0usize)).with(|state| *state);
    let timeline_started = remember(|| mutableStateOf(false)).with(|state| *state);
    let reference_page_override = remember(|| mutableStateOf(None::<usize>)).with(|state| *state);
    TAB_SWIPE_REFERENCE_PAGE_OVERRIDE.with(|cell| {
        *cell.borrow_mut() = Some(reference_page_override);
    });
    let timeline_progress = cranpose_animation::animateFloatAsState(
        if timeline_started.get() { 1.0 } else { 0.0 },
        cranpose_animation::tween(1_867, cranpose_animation::Easing::LinearEasing),
        "tab-reference-backdrop",
    );
    Box(
        Modifier::empty().width(440.0).height(132.0),
        BoxSpec::default(),
        move || {
            TabSwipeFixtureBackdrop(
                reference_page_override
                    .get()
                    .unwrap_or_else(|| tab_swipe_reference_page(timeline_progress.get())),
            );
            let tab_state = selected;
            let start_timeline = timeline_started;
            Box(
                Modifier::empty()
                    .fill_max_size()
                    .padding_each(0.0, 0.0, 0.0, 10.0),
                BoxSpec::default().content_alignment(Alignment::new(
                    HorizontalAlignment::CenterHorizontally,
                    VerticalAlignment::Bottom,
                )),
                move || {
                    let callback_state = tab_state;
                    LiquidTabBarWithAccessory(
                        Modifier::empty(),
                        LiquidTabBarSpec::default(),
                        tab_swipe_reference_tabs(),
                        tab_state.get(),
                        move |index| {
                            start_timeline.set(true);
                            callback_state.set(index);
                        },
                        || LiquidTabBarSearchAccessory(|| {}),
                    );
                },
            );
        },
    );
}

#[composable]
pub fn LiquidReferenceFixture(case: LiquidReferenceFixtureCase) {
    let background = match case {
        LiquidReferenceFixtureCase::MenuExpand => Color::from_rgb_u8(25, 8, 29),
        LiquidReferenceFixtureCase::OnWhiteClick | LiquidReferenceFixtureCase::OnWhiteClickHold => {
            Color::from_rgb_u8(241, 240, 247)
        }
        LiquidReferenceFixtureCase::OnWhiteTouchedUp => Color::from_rgb_u8(241, 240, 247),
        _ => Color::from_rgb_u8(245, 245, 251),
    };
    Box(
        Modifier::empty().fill_max_size().draw_behind(move |scope| {
            scope.draw_rect(Brush::solid(background));
        }),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || match case {
            LiquidReferenceFixtureCase::TogglePress => {
                let checked = remember(|| mutableStateOf(true)).with(|state| *state);
                LiquidTheme(
                    LiquidThemeSpec {
                        scheme: SchemeMode::Light,
                        accent: Color::from_rgb_u8(52, 199, 89),
                        ..LiquidThemeSpec::default()
                    },
                    move || {
                        let state = checked;
                        TogglePressReferenceStage(state.get(), move |value| state.set(value));
                    },
                );
            }
            LiquidReferenceFixtureCase::MenuOpen => {
                LiquidTheme(LiquidThemeSpec::default(), MenuOpenReferenceStage);
            }
            LiquidReferenceFixtureCase::TabSwipe => {
                LiquidTheme(LiquidThemeSpec::default(), TabSwipeReferenceStage);
            }
            LiquidReferenceFixtureCase::Segmented => {
                let selected = remember(|| mutableStateOf(0usize)).with(|state| *state);
                LiquidTheme(LiquidThemeSpec::default(), move || {
                    let state = selected;
                    LiquidSegmentedControl(
                        Modifier::empty().width(310.0),
                        vec![
                            "Receiving".to_string(),
                            "Sending".to_string(),
                            "Errored".to_string(),
                        ],
                        state.get(),
                        move |index| state.set(index),
                    );
                });
            }
            LiquidReferenceFixtureCase::MenuExpand => SortFilterFixtureStage(),
            LiquidReferenceFixtureCase::BottomBarForm => {
                let selected = remember(|| mutableStateOf(4usize)).with(|state| *state);
                LiquidTheme(LiquidThemeSpec::default(), move || {
                    let store_state = selected;
                    let headers_state = selected;
                    Column(Modifier::empty(), ColumnSpec::default(), move || {
                        StoreBottomBarExample(store_state.get(), move |index| {
                            store_state.set(index);
                        });
                        Box(Modifier::empty().height(24.0), BoxSpec::default(), || {});
                        StoreHeadersBarExample(headers_state.get(), move |index| {
                            headers_state.set(index);
                        });
                    });
                });
            }
            LiquidReferenceFixtureCase::OnWhiteClick
            | LiquidReferenceFixtureCase::OnWhiteClickHold => {
                let selected = remember(|| mutableStateOf(0usize)).with(|state| *state);
                let state = selected;
                OnWhiteBottomBarExample(state.get(), move |index| state.set(index));
            }
            LiquidReferenceFixtureCase::OnWhiteTouchedUp => TouchedUpFixtureStage(),
        },
    );
}

/// The showcase page.
#[composable]
pub fn LiquidUiTab() {
    let dark = remember(|| mutableStateOf(false)).with(|s| *s);
    let selected_tab = remember(|| mutableStateOf(0usize)).with(|s| *s);
    super::TEST_LIQUID_SELECTED_TAB_STATE.with(|cell| {
        *cell.borrow_mut() = Some(selected_tab);
    });
    let store_tab = remember(|| mutableStateOf(4usize)).with(|s| *s);
    let on_white_tab = remember(|| mutableStateOf(0usize)).with(|s| *s);
    let toggle_a = remember(|| mutableStateOf(true)).with(|s| *s);
    let toggle_b = remember(|| mutableStateOf(false)).with(|s| *s);
    let slider = remember(|| mutableStateOf(0.6f32)).with(|s| *s);
    let segment = remember(|| mutableStateOf(0usize)).with(|s| *s);
    let chip = remember(|| mutableStateOf(0usize)).with(|s| *s);
    let menu_open = remember(|| mutableStateOf(false)).with(|s| *s);
    let menu_mode = remember(|| mutableStateOf(0usize)).with(|s| *s);
    let menu_gesture = remember_liquid_menu_gesture();
    let clicks = remember(|| mutableStateOf(0u32)).with(|s| *s);
    let scheme = if dark.get() {
        SchemeMode::Dark
    } else {
        SchemeMode::Light
    };

    LiquidTheme(
        LiquidThemeSpec {
            scheme,
            ..LiquidThemeSpec::default()
        },
        move || {
            let menu_gesture = menu_gesture.clone();
            let colors = liquid_colors();
            let scroll = remember(|| ScrollState::new(0.0)).with(|s| *s);
            super::TEST_LIQUID_SCROLL_STATE.with(|cell| {
                *cell.borrow_mut() = Some(scroll);
            });
            // The menu anchors to the REAL composited rects of the Featured
            // videos card's trailing circles (window coords via
            // report_window_rect) — never guessed offsets.
            let menu_anchor_rect = cranpose_core::remember(|| {
                std::rc::Rc::new(std::cell::Cell::new(Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                }))
            })
            .with(std::rc::Rc::clone);
            let menu_absorbed_rect = cranpose_core::remember(|| {
                std::rc::Rc::new(std::cell::Cell::new(Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                }))
            })
            .with(std::rc::Rc::clone);
            let absorbed_button_spec = GlassButtonSpec::glass()
                .with_glass(
                    Glass::regular()
                        .lift(0.05)
                        .highlight(0.30)
                        .adaptive_frost(colors.label, 0.35),
                )
                .with_icon_backplate(colors.accent)
                .with_content_color(Color::WHITE);
            let stage_menu_gesture = menu_gesture.clone();
            let stage_anchor = std::rc::Rc::clone(&menu_anchor_rect);
            let stage_absorbed = std::rc::Rc::clone(&menu_absorbed_rect);
            let stage_button_spec = absorbed_button_spec.clone();

            cranpose::widgets::Box(
                Modifier::empty()
                    .semantics(|config: &mut SemanticsConfiguration| {
                        config.content_description = Some(LIQUID_SCROLL_VIEWPORT_TAG.to_string());
                    })
                    .fill_max_size()
                    .draw_behind(move |scope| {
                        scope.draw_rect(Brush::solid(colors.background));
                    }),
                cranpose::widgets::BoxSpec::default(),
                move || {
                    // ---- Scrollable content (slides under the bars) ----
                    let dark_for_chip = dark;
                    let toggle_a = toggle_a;
                    let toggle_b = toggle_b;
                    let slider_state = slider;
                    let segment_state = segment;
                    let chip_state = chip;
                    let store_tab_state = store_tab;
                    let on_white_tab_state = on_white_tab;
                    let clicks_state = clicks;
                    let stage_menu_gesture = stage_menu_gesture.clone();
                    let stage_anchor = std::rc::Rc::clone(&stage_anchor);
                    let stage_absorbed = std::rc::Rc::clone(&stage_absorbed);
                    let stage_button_spec = stage_button_spec.clone();
                    Column(
                        Modifier::empty()
                            .fill_max_size()
                            .vertical_scroll(scroll, false)
                            .padding_each(
                                PAGE_PADDING,
                                liquid_nav_bar_expanded_height() + 8.0,
                                PAGE_PADDING,
                                120.0,
                            ),
                        ColumnSpec::default(),
                        move || {
                            FeaturedVideosReferenceStage(
                                menu_open,
                                stage_menu_gesture.clone(),
                                std::rc::Rc::clone(&stage_anchor),
                                std::rc::Rc::clone(&stage_absorbed),
                                stage_button_spec.clone(),
                            );

                            SectionTitle("wcKSRD OPTICS");
                            OpticalShaderPreview();

                            SectionTitle("TOGGLE PRESS");
                            let reference_toggle = toggle_a;
                            TogglePressReferenceStage(reference_toggle.get(), move |value| {
                                reference_toggle.set(value)
                            });
                            Box(Modifier::empty().height(12.0), BoxSpec::default(), || {});

                            SectionTitle("TAB SWIPE");
                            TabSwipeReferenceBackdrop();
                            Box(Modifier::empty().height(12.0), BoxSpec::default(), || {});

                            // WWDC-style sessions list: the content the
                            // floating tab bar and morphing menu hover over.
                            for (icon, title, subtitle) in [
                                (
                                    icons::GRID,
                                    "iPadOS",
                                    "Unlock the full potential of iPadOS.",
                                ),
                                (
                                    icons::FOLDER,
                                    "macOS",
                                    "Take full advantage of the powerful capabilities of Mac.",
                                ),
                                (
                                    icons::STAR,
                                    "Metal",
                                    "Explore the latest in Metal tools, technologies, and resources.",
                                ),
                                (
                                    icons::EDIT,
                                    "Swift",
                                    "Discover updates to Swift and related tools and frameworks.",
                                ),
                                (
                                    icons::HOME,
                                    "SwiftUI",
                                    "Design and build your apps like never before.",
                                ),
                            ] {
                                SessionCard(icon, title, subtitle);
                            }

                            SectionTitle("BOTTOM BAR");
                            let store_state = store_tab_state;
                            StoreBottomBarExample(store_state.get(), move |index| {
                                store_state.set(index);
                            });
                            let headers_state = store_tab_state;
                            StoreHeadersBarExample(headers_state.get(), move |index| {
                                headers_state.set(index);
                            });

                            SectionTitle("SORT FILTER MENU");
                            SortFilterReferenceStage();
                            let on_white_state = on_white_tab_state;
                            OnWhiteBottomBarExample(on_white_state.get(), move |index| {
                                on_white_state.set(index);
                            });
                            Box(Modifier::empty().height(12.0), BoxSpec::default(), || {});

                            // Vivid backdrop content for the glass to lens.
                            GradientStripe(
                                [
                                    Color::from_rgb_u8(255, 94, 58),
                                    Color::from_rgb_u8(255, 149, 0),
                                ],
                                120.0,
                            );

                            SectionTitle("BUTTONS");
                            TouchedUpReferenceStage();
                            Box(Modifier::empty().height(8.0), BoxSpec::default(), || {});
                            Row(
                                Modifier::empty(),
                                RowSpec::default()
                                    .vertical_alignment(VerticalAlignment::CenterVertically),
                                {
                                    let clicks = clicks_state;
                                    move || {
                                        let clicks_a = clicks;
                                        GlassButton(
                                            Modifier::empty(),
                                            GlassButtonSpec::glass(),
                                            move || clicks_a.set(clicks_a.get() + 1),
                                            || {
                                                GlassButtonLabel("Glass", GlassButtonSpec::glass());
                                            },
                                        );
                                        Box(
                                            Modifier::empty().width(12.0),
                                            BoxSpec::default(),
                                            || {},
                                        );
                                        let clicks_b = clicks;
                                        GlassButton(
                                            Modifier::empty(),
                                            GlassButtonSpec::prominent(),
                                            move || clicks_b.set(clicks_b.get() + 1),
                                            || {
                                                GlassButtonLabel(
                                                    "Prominent",
                                                    GlassButtonSpec::prominent(),
                                                );
                                            },
                                        );
                                        Box(
                                            Modifier::empty().width(12.0),
                                            BoxSpec::default(),
                                            || {},
                                        );
                                        let clicks_c = clicks;
                                        GlassIconButton(
                                            Modifier::empty(),
                                            GlassButtonSpec::glass(),
                                            44.0,
                                            move || clicks_c.set(clicks_c.get() + 1),
                                            icons::PLUS,
                                        );
                                    }
                                },
                            );

                            SectionTitle("CONTROLS");
                            LiquidCard(Modifier::empty().fill_max_width(), {
                                let toggle_a = toggle_a;
                                let toggle_b = toggle_b;
                                let slider_state = slider_state;
                                move || {
                                    let toggle_a = toggle_a;
                                    let toggle_b = toggle_b;
                                    let slider_state = slider_state;
                                    Column(
                                        Modifier::empty().padding(16.0),
                                        ColumnSpec::default(),
                                        move || {
                                            let colors = liquid_colors();
                                            let toggle_a2 = toggle_a;
                                            Row(
                                                Modifier::empty().fill_max_width(),
                                                RowSpec::default().vertical_alignment(
                                                    VerticalAlignment::CenterVertically,
                                                ),
                                                move || {
                                                    Text(
                                                        "Wi-Fi",
                                                        Modifier::empty().weight(1.0),
                                                        body_style(colors.label),
                                                    );
                                                    let t = toggle_a2;
                                                    LiquidToggle(
                                                        Modifier::empty().semantics(|config| {
                                                            config.role =
                                                                Some(SemanticsWidgetRole::Button);
                                                            config.is_clickable = true;
                                                            config.content_description = Some(
                                                                "Wi-Fi settings switch".to_string(),
                                                            );
                                                        }),
                                                        t.get(),
                                                        move |value| t.set(value),
                                                    );
                                                },
                                            );
                                            Box(
                                                Modifier::empty().height(12.0),
                                                BoxSpec::default(),
                                                || {},
                                            );
                                            let toggle_b2 = toggle_b;
                                            Row(
                                                Modifier::empty().fill_max_width(),
                                                RowSpec::default().vertical_alignment(
                                                    VerticalAlignment::CenterVertically,
                                                ),
                                                move || {
                                                    Text(
                                                        "Airplane Mode",
                                                        Modifier::empty().weight(1.0),
                                                        body_style(colors.label),
                                                    );
                                                    let t = toggle_b2;
                                                    LiquidToggle(
                                                        Modifier::empty(),
                                                        t.get(),
                                                        move |value| t.set(value),
                                                    );
                                                },
                                            );
                                            Box(
                                                Modifier::empty().height(16.0),
                                                BoxSpec::default(),
                                                || {},
                                            );
                                            let s = slider_state;
                                            LiquidSlider(
                                                Modifier::empty().fill_max_width(),
                                                s.get(),
                                                move |value| s.set(value),
                                            );
                                        },
                                    );
                                }
                            });

                            SectionTitle("SEGMENTED");
                            let seg = segment_state;
                            // The reference segmented recording's exact labels
                            // (Transfers page) so cheatsheet frames align.
                            LiquidSegmentedControl(
                                // Reference control geometry: 900px over
                                // 2.89 px/dp = 310dp, cells 103dp.
                                Modifier::empty().width(310.0),
                                vec![
                                    "Receiving".to_string(),
                                    "Sending".to_string(),
                                    "Errored".to_string(),
                                ],
                                seg.get(),
                                move |index| seg.set(index),
                            );

                            SectionTitle("CHIPS");
                            Row(Modifier::empty(), RowSpec::default(), {
                                let chip_state = chip_state;
                                let dark_for_chip = dark_for_chip;
                                move || {
                                    for (index, label) in
                                        ["All", "Recent", "Favorites"].iter().enumerate()
                                    {
                                        let chip_state2 = chip_state;
                                        LiquidChip(
                                            Modifier::empty().padding_each(0.0, 0.0, 8.0, 0.0),
                                            chip_state.get() == index,
                                            move || chip_state2.set(index),
                                            *label,
                                        );
                                    }
                                    let dark2 = dark_for_chip;
                                    LiquidChip(
                                        Modifier::empty(),
                                        dark_for_chip.get(),
                                        move || dark2.set(!dark2.get()),
                                        "Dark mode",
                                    );
                                }
                            });

                            GradientStripe(
                                [
                                    Color::from_rgb_u8(0, 122, 255),
                                    Color::from_rgb_u8(88, 86, 214),
                                ],
                                120.0,
                            );

                            SectionTitle("LIST");
                            LiquidListSection(Modifier::empty().fill_max_width(), "Library", {
                                move || {
                                    for (index, (icon, title)) in [
                                        (icons::DOCUMENT, "Receipt 2026-06-30"),
                                        (icons::CAMERA, "Idea Marketi doo"),
                                        (icons::FOLDER, "Groceries"),
                                    ]
                                    .into_iter()
                                    .enumerate()
                                    {
                                        LiquidListRow(
                                            Modifier::empty(),
                                            LiquidListRowSpec::default().with_separator(index < 2),
                                            || {},
                                            move || {
                                                let colors = liquid_colors();
                                                Row(
                                                    Modifier::empty().fill_max_width(),
                                                    RowSpec::default().vertical_alignment(
                                                        VerticalAlignment::CenterVertically,
                                                    ),
                                                    move || {
                                                        icons::Icon(icon, 20.0, colors.accent);
                                                        Box(
                                                            Modifier::empty().width(12.0),
                                                            BoxSpec::default(),
                                                            || {},
                                                        );
                                                        Text(
                                                            title,
                                                            Modifier::empty().weight(1.0),
                                                            body_style(colors.label),
                                                        );
                                                        icons::Icon(
                                                            icons::CHEVRON_RIGHT,
                                                            16.0,
                                                            colors.tertiary_label,
                                                        );
                                                    },
                                                );
                                            },
                                        );
                                    }
                                }
                            });
                            GradientStripe(
                                [
                                    Color::from_rgb_u8(175, 82, 222),
                                    Color::from_rgb_u8(255, 204, 0),
                                ],
                                160.0,
                            );
                        },
                    );

                    // ---- Nav bar over the content ----
                    // The menu's anchor circles live in the Featured videos
                    // card header (the reference composition); the nav keeps
                    // only the title.
                    LiquidNavBar(
                        Modifier::empty().fill_max_width(),
                        LiquidNavBarSpec::new("WWDC"),
                        scroll,
                        || {},
                        || {},
                    );

                    // ---- Morphing menu ----
                    let menu_state = menu_open;
                    let menu_dismiss = menu_open;
                    let menu_selection = menu_mode;
                    let menu_anchor = menu_anchor_rect.get();
                    let menu_absorbed = menu_absorbed_rect.get();
                    LiquidMenu(
                        menu_state.get(),
                        menu_anchor,
                        LiquidMenuSpec::default(),
                        vec![
                            LiquidMenuAbsorbedSource::new(
                                menu_absorbed,
                                absorbed_button_spec.clone(),
                                36.0,
                                icons::FILTER,
                            ),
                            // The tapped "…" itself: its trigger backdrop
                            // unmounts instantly, so without a replica the
                            // glyph pops out within a frame — the reference
                            // keeps it readable under the growing droplet
                            // and ghosts it out gradually.
                            LiquidMenuAbsorbedSource::new(
                                menu_anchor,
                                GlassButtonSpec::glass(),
                                40.0,
                                icons::MORE_HORIZ,
                            ),
                        ],
                        vec![
                            LiquidMenuItem::new("List")
                                .icon(icons::LIST_OUTLINE)
                                .checked(menu_selection.get() == 0),
                            LiquidMenuItem::new("Grid")
                                .icon(icons::GRID_OUTLINE)
                                .checked(menu_selection.get() == 1),
                        ],
                        menu_gesture.clone(),
                        move |index| menu_selection.set(index),
                        move || menu_dismiss.set(false),
                    );

                    // ---- Floating tab bar ----
                    let tab_state = selected_tab;
                    Box(
                        Modifier::empty().fill_max_size().padding_each(
                            PAGE_PADDING,
                            0.0,
                            PAGE_PADDING,
                            LIQUID_TAB_BAR_BOTTOM_PADDING,
                        ),
                        BoxSpec::default().content_alignment(Alignment::new(
                            HorizontalAlignment::CenterHorizontally,
                            VerticalAlignment::Bottom,
                        )),
                        move || {
                            let tab_state2 = tab_state;
                            LiquidTabBarWithAccessory(
                                Modifier::empty(),
                                LiquidTabBarSpec::default(),
                                tab_swipe_reference_tabs(),
                                tab_state.get(),
                                move |index| tab_state2.set(index),
                                || {
                                    LiquidTabBarSearchAccessory(|| {});
                                },
                            );
                        },
                    );
                },
            );
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shader_preview_uses_a_bounded_live_wcksrd_lens() {
        assert_eq!(OPTICAL_PREVIEW_HEIGHT, 256.0);
        assert_eq!(OPTICAL_PREVIEW_LENS_SIZE, Size::new(150.0, 82.0));
        let stage = Size::new(440.0, OPTICAL_PREVIEW_HEIGHT);
        let initial = initial_optical_preview_center(stage);
        assert_eq!(initial.x, 220.0);
        assert!((initial.y - 71.68).abs() < 1.0e-4);
        assert_eq!(
            clamp_optical_preview_center(Point::new(-20.0, -20.0), stage),
            Point::new(75.0, 41.0)
        );
        assert_eq!(
            clamp_optical_preview_center(Point::new(900.0, 900.0), stage),
            Point::new(365.0, 215.0)
        );
    }

    #[test]
    fn shader_playground_maps_normalized_controls_to_wcksrd() {
        let glass = optical_preview_glass(0.50, 0.75, 0.25, 0.30, 0.34);
        assert_eq!(glass.refraction_depth, 0.50);
        assert_eq!(glass.refraction_curve, 0.75);
        assert_eq!(glass.blur_radius, Some(2.0));
        assert_eq!(glass.dispersion, 0.30);
        assert_eq!(glass.highlight, 0.34);
    }

    #[test]
    fn toggle_press_reference_stage_uses_target_pixels_at_three_x() {
        let layout = toggle_press_reference_layout();
        assert_eq!(layout.size, Size::new(340.0 / 3.0, 190.0 / 3.0));
        assert_eq!(layout.track_origin, Point::new(37.0 / 3.0, 52.0 / 3.0));
        assert_eq!(
            layout.backdrop_cap_center,
            Point::new(203.0 / 3.0, 94.0 / 3.0)
        );
        assert_eq!(layout.backdrop_cap_radius, 77.0 / 3.0);
        assert_eq!(layout.background, Color::from_rgb_u8(238, 237, 245));
    }

    #[test]
    fn tab_swipe_reference_backdrop_uses_target_geometry() {
        let layout = tab_swipe_reference_layout();
        assert_eq!(layout.size, Size::new(440.0, 132.0));
        assert_eq!(
            layout.title,
            Rect {
                x: 40.0,
                y: 0.0,
                width: 360.0,
                height: 29.0,
            }
        );
        assert_eq!(
            layout.enroll,
            Rect {
                x: 20.0,
                y: 33.0,
                width: 400.0,
                height: 51.0,
            }
        );
        assert_eq!(
            layout.xcode,
            Rect {
                x: 32.0,
                y: 69.0,
                width: 376.0,
                height: 50.0,
            }
        );
    }

    #[test]
    fn tab_swipe_reference_row_is_physical_glass() {
        let glass = tab_swipe_reference_glass();
        assert_eq!(glass.variant, GlassVariant::Regular);
        assert!(glass.shadow);
        assert_eq!(glass.refraction_depth, 0.34);
        assert!(glass.blur_radius.is_none());
        assert!(glass.tint.is_none());
    }

    #[test]
    fn floating_tab_bar_matches_the_reference_bottom_inset() {
        assert_eq!(LIQUID_TAB_BAR_BOTTOM_PADDING, 19.0);
    }

    #[test]
    fn tab_swipe_sources_match_the_reference_symbol_footprints() {
        let tabs = tab_swipe_reference_tabs();
        assert_eq!(tabs.len(), 4);
        assert_eq!(tabs[2].icon_style, LiquidTabIconStyle::AppBadge);
        assert_eq!(tabs[2].icon_scale, 0.94);
        assert_eq!(tabs[3].icon_scale, 0.95);
    }
}
