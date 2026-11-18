// The page section: a sheet of cotton paper lying in the sun under a tree.
// Its words are live composables printed by letterpress: the headline, the
// chosen language and the button are pressed into the paper, small type is
// printed flat. A form switches the language, which Cranpose's
// localization lays out from the right for Arabic, and the type's size;
// the press follows the words wherever they go.

use std::{
    cell::Cell,
    rc::Rc,
    sync::{Arc, OnceLock},
};

use cranpose_core::{remember, rememberMutableStateOf, CompositionLocalProvider, MutableState};
use cranpose_foundation::SemanticsWidgetRole;
use cranpose_services::local_uri_handler;
use cranpose_ui::{
    composable, local_text_selection_colors,
    space::{Light, Mat4, Material, Mesh, MeshSurface, Shaded, Vec3},
    text::{
        measure_text_with_options, AnnotatedString, TextDecoration, TextLayoutOptions, TextUnit,
    },
    tr, translations,
    widgets::{FlowRow, FlowSpec, SelectionContainer, Slider, SliderSpec},
    Box, BoxSpec, Catalog, Color, Column, ColumnSpec, LinearArrangement, LinkAnnotation,
    LinkedText, Locale, Modifier, ProvideLocalization, RoundedCornerShape, Row, RowSpec, Size,
    Text, TextSelectionColors, TextStyle, VerticalAlignment,
};

use super::{
    language::Language,
    lettering::{headline, strong, text},
    stage::SceneStage,
    tree::{Tree, SHADOW_REACH},
    walk::unit,
};
use crate::app::ticking::Ticking;

// How many window heights the section takes.
pub(super) const PAGE_PAGES: f32 = 1.0;
// The content's key colours; `cotton.wgsl` tells them apart.
const GROUND: Color = Color::WHITE;
const INK: Color = Color::BLACK;
const ACCENT: Color = Color(0.0, 1.0, 0.0, 1.0);
const BLIND: Color = Color(1.0, 1.0, 0.0, 1.0);
const WAX: Color = Color(0.0, 1.0, 1.0, 1.0);
const SELECTION: Color = Color(1.0, 0.0, 1.0, 1.0);
const REPOSITORY_URL: &str = "https://github.com/samoylenkodmitry/Cranpose";
const SOURCE_URL: &str =
    "https://github.com/samoylenkodmitry/Cranpose/tree/main/apps/desktop-demo/src/app/explainer";
const COTTON_WGSL: &str = cranpose_ui::include_wgsl!("noise.wgsl", "cotton.wgsl");
// The type's scale at either end of the size slider.
const SMALLEST_TYPE: f32 = 0.7;
const LARGEST_TYPE: f32 = 1.3;
// Seconds for new words to be pressed in after a switch of language.
const PRESS_SECONDS: f32 = 1.15;
// Fresh ink is glossy: how wet it is once pressed, and how fast it dries.
const WET_AFTER_PRESS: f32 = 0.5;
const DRY_RATE: f32 = 0.45;

// The sun, low in the upper left: its shadows fall right and down. Its
// disc is small: the boughs stand high over the sheet, out of the camera's
// view, and a wider one would blur their leaves away.
fn sun() -> Light {
    let (x, y) = SHADOW_REACH;
    let length = (x * x + y * y + 1.0).sqrt();
    Light {
        direction: Vec3::new(x / length, y / length, 1.0 / length),
        ambient: 0.5,
        shadows: true,
        size: 0.003,
    }
}

// What the reader chose.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Reading {
    language: Language,
    // Where the size slider stands, from 0 to 1.
    size: f32,
}

impl Reading {
    fn type_scale(self) -> f32 {
        SMALLEST_TYPE + (LARGEST_TYPE - SMALLEST_TYPE) * self.size
    }

    fn rtl(self) -> bool {
        self.language == Language::Arabic
    }
}

// The sheet's measures, in pixels of a stage of unit `u`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Measures {
    u: f32,
    scale: f32,
    language: Language,
}

impl Measures {
    fn pad_top(self) -> f32 {
        68.0 * self.u
    }

    fn pad_side(self, size: Size) -> f32 {
        let column = (size.width - 2.0 * (132.0 * self.u).max(26.0)).min(1176.0 * self.u);
        ((size.width - column) * 0.5).max(26.0)
    }

    fn headline(self) -> TextStyle {
        let (size, line) = match self.language {
            Language::Japanese => (124.0, 1.1),
            Language::Arabic => (150.0, 0.96),
            Language::English | Language::Spanish => (142.0, 0.96),
        };
        let mut style = headline(size * self.u * self.scale, INK);
        style.paragraph_style.line_height = TextUnit::Sp(size * line * self.u * self.scale);
        style
    }

    fn words(self) -> TextStyle {
        let (size, line) = match self.language {
            Language::Japanese => (28.0, 1.6),
            Language::Arabic => (34.0, 1.46),
            Language::English | Language::Spanish => (31.0, 1.42),
        };
        let mut style = text(size * self.u * self.scale, INK);
        style.paragraph_style.line_height = TextUnit::Sp(size * line * self.u * self.scale);
        style
    }

    fn words_width(self, column: f32) -> f32 {
        let ems = match self.language {
            Language::Japanese => 33.0 * 28.0,
            Language::Arabic => 28.0 * 34.0,
            Language::Spanish => 31.0 * 31.0,
            Language::English => 29.0 * 31.0,
        };
        (ems * self.u * self.scale).min(column)
    }

    fn text_top(self) -> f32 {
        (54.0 - (self.scale - 1.0) * 220.0).max(0.0) * self.u
    }

    fn headline_height(self) -> f32 {
        272.0 * self.u * self.scale
    }

    fn controls(self) -> TextStyle {
        strong(21.0 * self.u, INK)
    }
}

// The section, playing while `moving`.
#[composable(no_skip)]
pub(super) fn PageSection(size: Size, moving: bool) {
    let catalog =
        remember(|| translations!("src/app/explainer/locales", fallback = "en")).with(Clone::clone);
    let reading = rememberMutableStateOf(|| Reading {
        language: Language::English,
        size: 0.5,
    });
    PageStage(size, moving, (catalog, reading));
}

#[composable(no_skip)]
fn PageStage(size: Size, moving: bool, (catalog, reading): (Catalog, MutableState<Reading>)) {
    let u = unit(size);
    let breeze = rememberMutableStateOf(|| 40.0f32);
    // The press of the words and the ink's wetness: (pressed in, wet).
    let press = rememberMutableStateOf(|| (1.0f32, 0.0f32));
    let shown = remember(|| Rc::new(Cell::new(Language::English))).with(Rc::clone);
    Ticking(moving, move |seconds| {
        breeze.set(breeze.get_non_reactive() + seconds);
        let language = reading.get_non_reactive().language;
        let (pressed, wet) = press.get_non_reactive();
        let next = if shown.replace(language) != language {
            (0.0, 0.0)
        } else if pressed < 1.0 {
            let pressed = (pressed + seconds / PRESS_SECONDS).min(1.0);
            (pressed, if pressed >= 1.0 { WET_AFTER_PRESS } else { 0.0 })
        } else {
            (1.0, (wet - seconds * DRY_RATE).max(0.0))
        };
        if next != (pressed, wet) {
            press.set(next);
        }
    });
    let now = reading.value();
    let measures = Measures {
        u,
        scale: now.type_scale(),
        language: now.language,
    };
    SceneStage(size, Some(sun()), move || {
        Sheet(size, measures, (catalog.clone(), reading, press));
        Tree(size, u, move || breeze.get());
    });
}

#[composable(no_skip)]
fn Sheet(
    size: Size,
    measures: Measures,
    (catalog, reading, press): (Catalog, MutableState<Reading>, MutableState<(f32, f32)>),
) {
    let plane = remember(|| Arc::new(Mesh::plane())).with(Arc::clone);
    let headline_end = measures.pad_top() + measures.text_top() + measures.headline_height();
    let rtl = reading.value().rtl();
    let wash = TextSelectionColors {
        handle_color: SELECTION,
        background_color: SELECTION,
    };
    Box(
        Modifier::empty()
            .required_size(size)
            .mesh_surface(move || {
                let (pressed, wet) = press.get();
                MeshSurface {
                    mesh: Arc::clone(&plane),
                    transform: Mat4::scaling(Vec3::new(size.width, size.height, 1.0)),
                    material: cotton(size, measures.u, headline_end, (pressed, wet, rtl)),
                    casts_shadow: false,
                    solid: true,
                }
            })
            .background(GROUND)
            .padding_each(
                measures.pad_side(size),
                measures.pad_top(),
                measures.pad_side(size),
                86.0 * measures.u,
            ),
        BoxSpec::default(),
        move || {
            // Only the words take the language's direction: the stage and
            // the tree over it stay where they are.
            let locale = Locale::parse(measures.language.tag())
                .expect("the page's languages have valid tags");
            ProvideLocalization(&catalog, locale, move || {
                CompositionLocalProvider(
                    [local_text_selection_colors().provides(wash)],
                    move || {
                        SheetWords(size, measures, reading);
                    },
                );
            });
        },
    );
}

#[composable(no_skip)]
fn SheetWords(size: Size, measures: Measures, reading: MutableState<Reading>) {
    let u = measures.u;
    let column = size.width - 2.0 * measures.pad_side(size);
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            SelectionContainer(Modifier::empty().weight(1.0), move || {
                Column(
                    Modifier::empty().padding_each(0.0, measures.text_top(), 0.0, 16.0 * u),
                    ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(
                        30.0 * u * measures.scale,
                    )),
                    move || {
                        let headline_text = tr!("It is still real text", id = "page-headline");
                        let style = measures.headline();
                        let height = measure_text_with_options(
                            &AnnotatedString::from(headline_text.as_str()),
                            &style,
                            TextLayoutOptions::default(),
                            Some(column),
                        )
                        .height;
                        Text(
                            headline_text,
                            Modifier::empty().height(height.max(measures.headline_height())),
                            style,
                        );
                        Text(
                            tr!(
                            "Cranpose lays these words out, so a screen reader can speak them and \
                             you can select and copy them. The paper is a shader under live \
                             composables. Switch the language or the size and the paper follows.",
                            id = "page-words"
                        ),
                            Modifier::empty().width(measures.words_width(column)),
                            measures.words(),
                        );
                    },
                );
            });
            Controls(measures, reading);
            Foot(measures);
        },
    );
}

// The language chips and the size slider.
#[composable(no_skip)]
fn Controls(measures: Measures, reading: MutableState<Reading>) {
    let u = measures.u;
    FlowRow(
        Modifier::empty(),
        FlowSpec::new()
            .main_axis_spacing(52.0 * u)
            .cross_axis_spacing(18.0 * u),
        move || {
            Row(
                Modifier::empty(),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpacedBy(12.0 * u))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                move || {
                    Text(
                        tr!("Language", id = "page-language"),
                        Modifier::empty(),
                        measures.controls(),
                    );
                    for language in Language::ALL {
                        cranpose_core::key(language, move || {
                            LanguageChip(measures, language, reading);
                        });
                    }
                },
            );
            Row(
                Modifier::empty(),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpacedBy(18.0 * u))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                move || {
                    Text(
                        tr!("Type size", id = "page-type-size"),
                        Modifier::empty(),
                        measures.controls(),
                    );
                    SizeSlider(measures, reading);
                },
            );
        },
    );
}

// A chip that switches to `language`: pressed in red while chosen, a blind
// outline otherwise.
#[composable(no_skip)]
fn LanguageChip(measures: Measures, language: Language, reading: MutableState<Reading>) {
    let u = measures.u;
    let chosen = reading.value().language == language;
    let em = 21.0 * u;
    let shape = RoundedCornerShape::uniform(10.0 * u);
    let (fill, label) = if chosen {
        (ACCENT, BLIND)
    } else {
        (GROUND, INK)
    };
    let outline = if chosen { ACCENT } else { BLIND };
    Box(
        Modifier::empty()
            .rounded_corners(10.0 * u)
            .background(fill)
            .border(3.0 * u, outline, shape)
            .selectable(chosen, Some(SemanticsWidgetRole::RadioButton), move || {
                reading.update(|now| now.language = language);
            })
            .padding_symmetric(0.9 * em, 0.46 * em),
        BoxSpec::default(),
        move || {
            let mut style = measures.controls();
            style.span_style.color = Some(label);
            Text(language.name(), Modifier::empty(), style);
        },
    );
}

// The type size: a blind groove with a raised disc of wax.
#[composable(no_skip)]
fn SizeSlider(measures: Measures, reading: MutableState<Reading>) {
    let u = measures.u;
    let thumb = 36.0 * u;
    let rtl = reading.value().rtl();
    Slider(
        Modifier::empty().size_points(236.0 * u, 44.0 * u),
        reading.value().size,
        move |value| reading.update(|now| now.size = value),
        || {},
        SliderSpec::new().thumb_extent(thumb).reverse_direction(rtl),
        move |scope| {
            Box(
                Modifier::empty().fill_max_size(),
                BoxSpec::default().content_alignment(cranpose_ui::Alignment::CENTER_START),
                move || {
                    Box(
                        Modifier::empty()
                            .fill_max_width()
                            .height(11.0 * u)
                            .rounded_corners(5.5 * u)
                            .background(BLIND),
                        BoxSpec::default(),
                        || {},
                    );
                    Box(
                        Modifier::empty()
                            .offset(scope.thumb_offset(), 0.0)
                            .size_points(thumb, thumb)
                            .rounded_corners(thumb * 0.5)
                            .background(WAX),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        },
    );
}

// The footnote with the button.
#[composable(no_skip)]
fn Foot(measures: Measures) {
    let u = measures.u;
    let uri_handler = local_uri_handler().current();
    Row(
        Modifier::empty().padding_each(0.0, 24.0 * u, 0.0, 0.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpacedBy(24.0 * u))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            let em = 23.0 * u;
            let opener = Rc::clone(&uri_handler);
            Box(
                Modifier::empty()
                    .rounded_corners(10.0 * u)
                    .background(ACCENT)
                    .clickable(move |_| open(&opener, SOURCE_URL))
                    .padding_symmetric(1.1 * em, 0.56 * em),
                BoxSpec::default(),
                move || {
                    Text(
                        tr!("Make your own", id = "page-button"),
                        Modifier::empty(),
                        strong(em, BLIND),
                    );
                },
            );
            let opener = Rc::clone(&uri_handler);
            let mut style = text(19.0 * u, INK);
            style.paragraph_style.line_height = TextUnit::Sp(19.0 * 1.36 * u);
            let mut link = strong(19.0 * u, INK).span_style;
            link.text_decoration = Some(TextDecoration::UNDERLINE);
            // The link follows the footnote in its last line, as one
            // paragraph a screen reader reads with its link action.
            let words = AnnotatedString::builder()
                .append(&tr!(
                    "Every scene in this tab is made of composables drawn on meshes and \
                     shaded with WGSL. The same code runs on Android, iOS, the desktop and \
                     the web.",
                    id = "page-footnote"
                ))
                .append(" ")
                .with_link(LinkAnnotation::Url(REPOSITORY_URL.into()), |builder| {
                    builder
                        .push_style(link)
                        .append(&tr!("Cranpose on GitHub", id = "page-link"))
                        .pop()
                })
                .to_annotated_string();
            LinkedText(words, Modifier::empty().weight(1.0), style, move |url| {
                open(&opener, url);
            });
        },
    );
}

fn open(uri_handler: &cranpose_services::UriHandlerRef, url: &str) {
    if let Err(error) = uri_handler.open_uri(url) {
        log::warn!("could not open {url}: {error}");
    }
}

// The sheet's paper for a stage of `size` and unit `u`, its headline
// pressed down to `headline_end`.
fn cotton(
    size: Size,
    u: f32,
    headline_end: f32,
    (pressed, wet, rtl): (f32, f32, bool),
) -> Material {
    static SHADER: OnceLock<Arc<str>> = OnceLock::new();
    let mut params = [0.0; Shaded::PARAMS];
    params[..9].copy_from_slice(&[
        size.width,
        size.height,
        u,
        24.0 * u,
        headline_end,
        pressed,
        wet,
        f32::from(u8::from(rtl)),
        1.0,
    ]);
    Material::Shaded(Shaded {
        shader: Arc::clone(SHADER.get_or_init(|| Arc::from(COTTON_WGSL))),
        params,
    })
}
