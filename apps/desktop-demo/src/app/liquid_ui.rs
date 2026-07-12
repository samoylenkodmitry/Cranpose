//! Liquid UI showcase: the full glass component set over colorful scrolling
//! content — buttons, toggle, slider, segmented control, chips, list cards,
//! the floating tab bar with its liquid selection blob, the morphing menu,
//! and a materials lab with live lens parameters.

#![allow(non_snake_case)]

use cranpose::liquid::prelude::*;
use cranpose::text::{SpanStyle, TextStyle, TextUnit};
use cranpose::widgets::{Box, BoxSpec, Column, ColumnSpec, Row, RowSpec, Text};
use cranpose::{
    composable, mutableStateOf, remember, Brush, Color, CornerRadii, Modifier, Point, Rect,
    ScrollState,
};
use cranpose_animation::{animateFloatAsState, tween, AnimationSpec, AnimationType, Easing};
use cranpose_ui::{Alignment, HorizontalAlignment, VerticalAlignment};

const PAGE_PADDING: f32 = 18.0;

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

/// The showcase page.
#[composable]
pub fn LiquidUiTab() {
    let dark = remember(|| mutableStateOf(false)).with(|s| *s);
    let selected_tab = remember(|| mutableStateOf(0usize)).with(|s| *s);
    let toggle_a = remember(|| mutableStateOf(true)).with(|s| *s);
    let toggle_b = remember(|| mutableStateOf(false)).with(|s| *s);
    let slider = remember(|| mutableStateOf(0.6f32)).with(|s| *s);
    let segment = remember(|| mutableStateOf(0usize)).with(|s| *s);
    let chip = remember(|| mutableStateOf(0usize)).with(|s| *s);
    let menu_open = remember(|| mutableStateOf(false)).with(|s| *s);
    let clicks = remember(|| mutableStateOf(0u32)).with(|s| *s);
    // Materials lab parameters.
    let lab_displacement = remember(|| mutableStateOf(0.5f32)).with(|s| *s);
    let lab_aberration = remember(|| mutableStateOf(0.4f32)).with(|s| *s);
    let lab_blur = remember(|| mutableStateOf(0.55f32)).with(|s| *s);

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
            let colors = liquid_colors();
            let scroll = remember(|| ScrollState::new(0.0)).with(|s| s.clone());
            let scroll_for_bar = scroll.clone();

            cranpose::widgets::Box(
                Modifier::empty().fill_max_size().draw_behind(move |scope| {
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
                    let clicks_state = clicks;
                    let lab_displacement = lab_displacement;
                    let lab_aberration = lab_aberration;
                    let lab_blur = lab_blur;
                    Column(
                        Modifier::empty()
                            .fill_max_size()
                            .vertical_scroll(scroll.clone(), false)
                            .padding_each(
                                PAGE_PADDING,
                                liquid_nav_bar_expanded_height() + 8.0,
                                PAGE_PADDING,
                                120.0,
                            ),
                        ColumnSpec::default(),
                        move || {
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

                            // Vivid backdrop content for the glass to lens.
                            GradientStripe(
                                [
                                    Color::from_rgb_u8(255, 94, 58),
                                    Color::from_rgb_u8(255, 149, 0),
                                ],
                                120.0,
                            );

                            SectionTitle("BUTTONS");
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
                                                        Modifier::empty(),
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
                            LiquidSegmentedControl(
                                Modifier::empty().fill_max_width(),
                                vec![
                                    "All".to_string(),
                                    "Receipts".to_string(),
                                    "Docs".to_string(),
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

                            SectionTitle("MATERIALS LAB");
                            // A glass tile over a rainbow strip, its lens
                            // driven by the sliders below.
                            let lab_d = lab_displacement;
                            let lab_a = lab_aberration;
                            let lab_b = lab_blur;
                            Box(
                                Modifier::empty().fill_max_width().height(150.0),
                                BoxSpec::default().content_alignment(Alignment::CENTER),
                                move || {
                                    GradientStripe(
                                        [
                                            Color::from_rgb_u8(52, 199, 89),
                                            Color::from_rgb_u8(255, 45, 85),
                                        ],
                                        150.0,
                                    );
                                    // Fine print behind the glass: high-frequency
                                    // detail that makes lensing and chromatic
                                    // aberration visible.
                                    Column(
                                        Modifier::empty().padding(10.0),
                                        ColumnSpec::default(),
                                        || {
                                            for _ in 0..7 {
                                                Text(
                                                    "the quick brown fox jumps over the lazy dog 0123456789 \
                                                     the quick brown fox jumps over the lazy dog",
                                                    Modifier::empty(),
                                                    body_style(Color::from_rgba_u8(20, 20, 24, 200)),
                                                );
                                            }
                                        },
                                    );
                                    GlassSurface(
                                        Modifier::empty().padding_symmetric(28.0, 18.0),
                                        Glass::regular()
                                            .displacement(40.0 * lab_d.get())
                                            .chromatic_aberration(lab_a.get())
                                            .blur_radius(24.0 * lab_b.get()),
                                        move || {
                                            let colors = liquid_colors();
                                            Text(
                                                "Liquid Glass",
                                                Modifier::empty(),
                                                heading_style(colors.label),
                                            );
                                        },
                                    );
                                },
                            );
                            LiquidCard(Modifier::empty().fill_max_width(), {
                                let lab_displacement = lab_displacement;
                                let lab_aberration = lab_aberration;
                                let lab_blur = lab_blur;
                                move || {
                                    let lab_displacement = lab_displacement;
                                    let lab_aberration = lab_aberration;
                                    let lab_blur = lab_blur;
                                    Column(
                                        Modifier::empty().padding(16.0),
                                        ColumnSpec::default(),
                                        move || {
                                            let colors = liquid_colors();
                                            for (label, state) in [
                                                ("Lens", lab_displacement),
                                                ("Aberration", lab_aberration),
                                                ("Frost", lab_blur),
                                            ] {
                                                Text(
                                                    label,
                                                    Modifier::empty(),
                                                    body_style(colors.secondary_label),
                                                );
                                                let s = state;
                                                LiquidSlider(
                                                    Modifier::empty().fill_max_width(),
                                                    s.get(),
                                                    move |value| s.set(value),
                                                );
                                            }
                                        },
                                    );
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
                    // The menu anchors to the REAL composited rects of the
                    // trailing buttons (window coords via report_window_rect)
                    // — never guessed offsets.
                    let menu_anchor_rect = cranpose_core::remember(|| {
                        std::rc::Rc::new(std::cell::Cell::new(Rect {
                            x: 0.0,
                            y: 0.0,
                            width: 0.0,
                            height: 0.0,
                        }))
                    })
                    .with(std::rc::Rc::clone);
                    let filter_rect = cranpose_core::remember(|| {
                        std::rc::Rc::new(std::cell::Cell::new(Rect {
                            x: 0.0,
                            y: 0.0,
                            width: 0.0,
                            height: 0.0,
                        }))
                    })
                    .with(std::rc::Rc::clone);
                    let menu_for_nav = menu_open;
                    let anchor_sink = std::rc::Rc::clone(&menu_anchor_rect);
                    let filter_sink = std::rc::Rc::clone(&filter_rect);
                    LiquidNavBar(
                        Modifier::empty().fill_max_width(),
                        LiquidNavBarSpec::new("WWDC"),
                        scroll_for_bar.clone(),
                        || {},
                        move || {
                            let menu = menu_for_nav;
                            let anchor_sink = std::rc::Rc::clone(&anchor_sink);
                            let filter_sink = std::rc::Rc::clone(&filter_sink);
                            Row(
                                Modifier::empty(),
                                RowSpec::default()
                                    .vertical_alignment(VerticalAlignment::CenterVertically),
                                move || {
                                    // The buttons the menu swallows fade out
                                    // as the droplet covers them (the settled
                                    // reference card shows ZERO trace of the
                                    // buttons beneath — a lingering blur
                                    // smudge reads as a glitch); they return
                                    // quickly on dismiss.
                                    let covered = animateFloatAsState(
                                        if menu.get() { 0.0 } else { 1.0 },
                                        if menu.get() {
                                            AnimationType::Tween(
                                                AnimationSpec::tween(180, Easing::EaseOut)
                                                    .with_delay(160),
                                            )
                                        } else {
                                            tween(140, Easing::EaseOut)
                                        },
                                        "nav-covered",
                                    );
                                    let covered_alpha = move || covered.get().clamp(0.0, 1.0);
                                    // Their GLASS is a backdrop effect and
                                    // ignores layer alpha, so once faded the
                                    // buttons UNMOUNT entirely (a fixed box
                                    // keeps the layout) — anything less
                                    // leaves the prominent button's blue
                                    // glass glowing through the card.
                                    if covered_alpha() > 0.02 {
                                        // The filter neighbor the menu bubble
                                        // glues past while growing (reference
                                        // keyframes 4-6): a blue prominent
                                        // circle like the reference filter.
                                        GlassIconButton(
                                            Modifier::empty()
                                                .report_window_rect(std::rc::Rc::clone(
                                                    &filter_sink,
                                                ))
                                                .graphics_layer(move || cranpose::GraphicsLayer {
                                                    alpha: covered_alpha(),
                                                    ..Default::default()
                                                }),
                                            GlassButtonSpec::prominent(),
                                            44.0,
                                            || {},
                                            icons::FILTER,
                                        );
                                        Box(
                                            Modifier::empty().width(8.0),
                                            BoxSpec::default(),
                                            || {},
                                        );
                                        GlassIconButton(
                                            Modifier::empty()
                                                .report_window_rect(std::rc::Rc::clone(
                                                    &anchor_sink,
                                                ))
                                                .graphics_layer(move || cranpose::GraphicsLayer {
                                                    alpha: covered_alpha(),
                                                    ..Default::default()
                                                }),
                                            GlassButtonSpec::glass(),
                                            44.0,
                                            move || menu.set(true),
                                            icons::MORE_HORIZ,
                                        );
                                    } else {
                                        Box(
                                            Modifier::empty().width(44.0 + 8.0 + 44.0).height(44.0),
                                            BoxSpec::default(),
                                            || {},
                                        );
                                    }
                                },
                            );
                        },
                    );

                    // ---- Morphing menu ----
                    let menu_state = menu_open;
                    let menu_dismiss = menu_open;
                    let menu_anchor = menu_anchor_rect.get();
                    let filter_neighbor = filter_rect.get();
                    LiquidMenu(
                        menu_state.get(),
                        menu_anchor,
                        vec![filter_neighbor],
                        vec![
                            LiquidMenuItem::header("Show"),
                            LiquidMenuItem::new("All Items").icon(icons::GRID),
                            LiquidMenuItem::new("Only Unwatched")
                                .icon(icons::EYE)
                                .checked(true)
                                .section_start(),
                            LiquidMenuItem::new("Only Bookmarked").icon(icons::BOOKMARK),
                            LiquidMenuItem::new("Delete All")
                                .icon(icons::TRASH)
                                .destructive()
                                .section_start(),
                        ],
                        |_index| {},
                        move || menu_dismiss.set(false),
                    );

                    // ---- Floating tab bar ----
                    let tab_state = selected_tab;
                    Box(
                        Modifier::empty().fill_max_size().padding_each(
                            PAGE_PADDING,
                            0.0,
                            PAGE_PADDING,
                            16.0,
                        ),
                        BoxSpec::default().content_alignment(Alignment::new(
                            HorizontalAlignment::CenterHorizontally,
                            VerticalAlignment::Bottom,
                        )),
                        move || {
                            let tab_state2 = tab_state;
                            LiquidTabBar(
                                Modifier::empty(),
                                vec![
                                    LiquidTab::new(icons::STAR, "Discover"),
                                    LiquidTab::new(icons::GRID, "Browse"),
                                    LiquidTab::new(icons::HOME, "Home"),
                                    LiquidTab::new(icons::SETTINGS, "Settings"),
                                ],
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
