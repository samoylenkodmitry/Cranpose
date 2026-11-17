// The silk section. It opens on the old choice: a card of live text beside
// a small curtain whose words are only a drawing. Its stage then stays in
// view while the scroll walks the section: the drawn words break into dots
// and leave, the title goes, the curtain grows across the stage over the
// card, and live words, a button and a link come up on it, which still work
// on the moving folds.

use std::{
    rc::Rc,
    sync::{Arc, OnceLock},
};

use cranpose_core::{derivedStateOf, remember, CompositionLocalProvider};
use cranpose_services::{local_uri_handler, UriHandlerRef};
use cranpose_ui::{
    composable, local_text_selection_colors, space::Vec3, text::AnnotatedString,
    widgets::SelectionContainer, Box, BoxSpec, Brush, Button, ButtonSpec, Canvas, Color, Column,
    ColumnSpec, CompositingStrategy, DrawTextStyle, DrawTransform, GraphicsLayer,
    LinearArrangement, LinkAnnotation, LinkedText, Modifier, Point, RenderEffect, Row, RowSpec,
    RuntimeShader, ScrollState, Size, Text, TextSelectionColors, VerticalAlignment,
};

use super::{
    card::WidgetCard,
    cloth::ClothHang,
    curtain::{Curtain, Gust, Mount, MountNow, SoakNow, LEAF_GOLD},
    lettering::{headline, strong, DISPLAY_FAMILY, STRONG_FAMILY},
    stage::{SceneStage, Wall, STAGE_LIGHT},
    walk::{ease, unit, Walk},
};

// How many window heights the section takes: one page, then the walk.
pub(super) const SILK_PAGES: f32 = 2.6;
const WALL: Color = Color(0.886, 0.882, 0.910, 1.0);
const NAVY: Color = Color(0.10, 0.12, 0.26, 1.0);
const CREAM: Color = Color(0.95, 0.91, 0.84, 1.0);
const BUTTON_INK: Color = Color(0.25, 0.16, 0.06, 1.0);
const REPOSITORY_URL: &str = "https://github.com/samoylenkodmitry/Cranpose";
// How deep the clips hold the cloth in front of the wall.
const CLOTH_DEPTH: f32 = -42.0;
// How deep the card's face stands in front of the wall. The cloth hangs in
// front of the card, never behind it.
const CARD_DEPTH: f32 = -12.0;
// The halftone's cell, in pixels as the small curtain shows them.
const DOT: f32 = 7.0;
const HALFTONE_WGSL: &str = concat!(include_str!("noise.wgsl"), include_str!("halftone.wgsl"));

// The section, `top` pixels down the article that `scroll` scrolls.
#[composable(no_skip)]
pub(super) fn SilkSection(size: Size, scroll: ScrollState, top: f32, moving: bool) {
    let walk = Walk::new(scroll, top, size.height, SILK_PAGES);
    let gust = remember(Gust::default).with(Clone::clone);
    Box(
        Modifier::empty()
            .required_size(Size::new(size.width, size.height * SILK_PAGES))
            .test_tag("silk"),
        BoxSpec::default(),
        move || {
            let gust = gust.clone();
            Box(
                Modifier::empty()
                    .required_size(size)
                    .graphics_layer_block(move |layer| layer.translation_y = walk.hold()),
                BoxSpec::default(),
                move || SilkStage(size, walk, (moving, gust.clone())),
            );
        },
    );
}

#[composable(no_skip)]
fn SilkStage(size: Size, walk: Walk, (moving, gust): (bool, Gust)) {
    let u = unit(size);
    let hang = curtain_hang(size);
    let (small, wide) = (small_mount(size, u, &hang), wide_mount(u, &hang));
    let drawn = derivedStateOf(move || walk.progress() < 0.4).get();
    let printed = derivedStateOf(move || walk.progress() > 0.6).get();
    SceneStage(size, Some(STAGE_LIGHT), move || {
        Wall(size, WALL);
        Column(
            Modifier::empty()
                .required_size(Size::new(size.width * 0.44, size.height))
                .padding_each(size.width * 0.06, size.height * 0.2, 0.0, 0.0),
            ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(40.0 * u)),
            move || {
                Text(
                    "Until now, an app had to choose",
                    Modifier::empty().graphics_layer_block(move |layer| {
                        let going = ease(0.12, 0.32, walk.progress());
                        layer.alpha = 1.0 - going;
                        layer.translation_y = -60.0 * u * going;
                    }),
                    headline(64.0 * u, NAVY),
                );
                WidgetCard(
                    Size::new(size.width * 0.38, size.height * 0.34),
                    (u, CARD_DEPTH, NAVY),
                    move || size.width * 0.05 * ease(0.25, 0.6, walk.progress()),
                );
            },
        );
        let mount: MountNow = Rc::new(move || small.lerp(wide, ease(0.3, 0.78, walk.progress())));
        // The drawn words are printed from the start; the live ones soak in
        // once the curtain has grown.
        let soak: SoakNow = Rc::new(move || {
            let progress = walk.progress();
            if progress < 0.5 {
                1.0
            } else {
                ease(0.62, 0.95, progress)
            }
        });
        let gust = gust.clone();
        Curtain(hang, (mount, soak), (moving, gust.clone()), move || {
            let gust = gust.clone();
            Box(
                Modifier::empty().fill_max_size(),
                BoxSpec::default(),
                move || {
                    if drawn {
                        DrawnWords(u, small.scale, walk);
                    }
                    if printed {
                        CurtainWords(hang.content, u, gust.clone());
                    }
                },
            );
        });
    });
}

// Words painted onto the small curtain: a picture of text, which no
// pointer can select. As the scroll walks on they break into dots, and the
// dots leave. They are painted against the curtain's `scale`, so they show
// upright on the small curtain.
#[composable(no_skip)]
fn DrawnWords(u: f32, scale: (f32, f32), walk: Walk) {
    Canvas(
        Modifier::empty().fill_max_size().graphics_layer(move || {
            let mut shader = RuntimeShader::from_shared_source(halftone_source());
            shader.set_float(0, ease(0.03, 0.34, walk.progress()));
            shader.set_float2(1, DOT / scale.0, DOT / scale.1);
            GraphicsLayer {
                render_effect: Some(RenderEffect::runtime_shader(shader)),
                compositing_strategy: CompositingStrategy::Offscreen,
                ..GraphicsLayer::default()
            }
        }),
        move |scope| {
            let unscale = DrawTransform::Scale {
                scale_x: 1.0 / scale.0,
                scale_y: 1.0 / scale.1,
                pivot: Point::new(0.0, 0.0),
            };
            scope.with_transform(&[unscale], &mut |scope| {
                let mut title = DrawTextStyle::default();
                title.font_family = Some(DISPLAY_FAMILY.to_owned());
                title.font_size = 58.0 * u;
                scope.draw_text_from(
                    Point::new(56.0 * u, 52.0 * u),
                    Brush::solid(CREAM),
                    "Canvas",
                    &title,
                );
                let mut words = DrawTextStyle::default();
                words.font_family = Some(STRONG_FAMILY.to_owned());
                words.font_size = 30.0 * u;
                words.line_height = Some(40.0 * u);
                for (line, row) in [
                    "Any picture you like, but",
                    "you draw everything",
                    "yourself. These words are",
                    "a drawing. Try to select",
                    "them.",
                ]
                .into_iter()
                .zip(0..)
                {
                    scope.draw_text_from(
                        Point::new(56.0 * u, (150.0 + 42.0 * row as f32) * u),
                        Brush::solid(CREAM),
                        line,
                        &words,
                    );
                }
            });
        },
    );
}

// The halftone shader's source, made once.
fn halftone_source() -> Arc<str> {
    static SOURCE: OnceLock<Arc<str>> = OnceLock::new();
    Arc::clone(SOURCE.get_or_init(|| {
        Arc::from(format!(
            "{}{HALFTONE_WGSL}",
            cranpose_ui_graphics::RUNTIME_SHADER_PRELUDE_WGSL
        ))
    }))
}

// The words printed on the grown curtain, laid out on the flat cloth.
// Selected words are laid in gold leaf.
#[composable(no_skip)]
fn CurtainWords(content: Size, u: f32, gust: Gust) {
    let uri_handler = local_uri_handler().current();
    let leaf = TextSelectionColors {
        handle_color: LEAF_GOLD,
        background_color: LEAF_GOLD,
    };
    CompositionLocalProvider([local_text_selection_colors().provides(leaf)], move || {
        CurtainSelection(content, u, (gust.clone(), Rc::clone(&uri_handler)));
    });
}

#[composable(no_skip)]
fn CurtainSelection(content: Size, u: f32, (gust, uri_handler): (Gust, UriHandlerRef)) {
    SelectionContainer(Modifier::empty().fill_max_size(), move || {
        let gust = gust.clone();
        let uri_handler = Rc::clone(&uri_handler);
        Column(
            Modifier::empty().required_size(content).padding_each(
                content.width * 0.16,
                90.0 * u,
                content.width * 0.36,
                0.0,
            ),
            ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(34.0 * u)),
            move || {
                Text(
                    "In Cranpose you\ndon't have to",
                    Modifier::empty(),
                    headline(104.0 * u, CREAM),
                );
                Text(
                    "Any composable can be drawn on a mesh. A cloth simulation moves \
                         this silk. Its words, button and link are still composables, and \
                         they work.",
                    Modifier::empty(),
                    strong(36.0 * u, CREAM),
                );
                let gust = gust.clone();
                let uri_handler = Rc::clone(&uri_handler);
                Row(
                    Modifier::empty(),
                    RowSpec::new()
                        .horizontal_arrangement(LinearArrangement::SpacedBy(44.0 * u))
                        .vertical_alignment(VerticalAlignment::CenterVertically),
                    move || {
                        let gust = gust.clone();
                        Button(
                            Modifier::empty()
                                .rounded_corners(36.0 * u)
                                .background(LEAF_GOLD)
                                .padding_symmetric(30.0 * u, 16.0 * u),
                            ButtonSpec::default(),
                            move || gust.blow(),
                            move || {
                                Text(
                                    "Send a gust",
                                    Modifier::empty(),
                                    strong(30.0 * u, BUTTON_INK),
                                );
                            },
                        );
                        let uri_handler = Rc::clone(&uri_handler);
                        let link = AnnotatedString::builder()
                            .with_link(LinkAnnotation::Url(REPOSITORY_URL.into()), |builder| {
                                builder.append("Cranpose on GitHub")
                            })
                            .to_annotated_string();
                        let mut style = strong(32.0 * u, LEAF_GOLD);
                        style.span_style.text_decoration =
                            Some(cranpose_ui::text::TextDecoration::UNDERLINE);
                        LinkedText(link, Modifier::empty(), style, move |url| {
                            if let Err(error) = uri_handler.open_uri(url) {
                                log::warn!("could not open {url}: {error}");
                            }
                        });
                    },
                );
            },
        );
    });
}

// The cloth the curtain is made of: as it hangs across the whole stage,
// its left edge past the stage's.
fn curtain_hang(size: Size) -> ClothHang {
    let span = size.width * 0.96;
    ClothHang {
        content: Size::new(span * 1.1, size.height * 0.88),
        top_left: Vec3::new(-size.width * 0.03, size.height * 0.06, CLOTH_DEPTH),
        wall: CARD_DEPTH,
        span,
        clips: 8,
        columns: 52,
        rows: 32,
    }
}

// The curtain small beside the card on the first page.
fn small_mount(size: Size, u: f32, hang: &ClothHang) -> Mount {
    Mount {
        at: Vec3::new(size.width * 0.5, size.height * 0.44, CLOTH_DEPTH),
        scale: (
            size.width * 0.42 / hang.span,
            size.height * 0.5 / hang.content.height,
        ),
        rod_radius: 4.0 * u,
        overhang: 20.0 * u,
    }
}

// The curtain grown across the stage, as its cloth hangs.
fn wide_mount(u: f32, hang: &ClothHang) -> Mount {
    Mount {
        at: hang.top_left,
        scale: (1.0, 1.0),
        rod_radius: 5.0 * u,
        overhang: 40.0 * u,
    }
}
