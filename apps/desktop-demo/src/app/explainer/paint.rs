// The title page: an oil painting of the page. The headline and the text
// are painted in thick strokes and lit by a lamp that rakes across them and
// leans after the pointer. The words stay live: they select, the links open
// and the button, a painted slab, scrolls the article on. A selection is a
// wet orange stroke dragged over the dry paint, its letters painted again
// in dark blue.

use cranpose_core::{mutableStateOf, remember, CompositionLocalProvider, MutableState, State};
use cranpose_services::local_uri_handler;
use cranpose_ui::{
    composable, local_text_selection_colors, rememberMutableInteractionSource,
    text::{annotated_string::Builder, AnnotatedString, SpanStyle, TextDecoration, TextUnit},
    widgets::SelectionContainer,
    Box, BoxSpec, Button, ButtonSpec, Column, ColumnSpec, LinearArrangement, LinkAnnotation,
    LinkedText, Modifier, MutableInteractionSource, Row, RowSpec, ScrollState, Size, Text,
    TextSelectionColors, TextStyle, VerticalAlignment,
};

use super::{
    lettering::{headline, strong},
    oil::{
        lit, painted, ButtonLook, OilPage, ACCENT_INK, BODY_INK, HEAD_INK, LABEL, PAGE, SELECTION,
        SLAB,
    },
    paint_lamp::rememberLamp,
};

// How many window heights the section takes.
pub(super) const PAINT_PAGES: f32 = 1.0;
const REPOSITORY_URL: &str = "https://github.com/samoylenkodmitry/Cranpose";
const COMPOSE_URL: &str = "https://developer.android.com/compose";
const HEAD_LINE: f32 = 0.98;
// A stage narrower than this sets the page in one column.
const NARROW: f32 = 700.0;

// How the page is set on a stage: its unit, type sizes, margins and gaps,
// the sizes in the unit.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Setting {
    u: f32,
    one_column: bool,
    head_em: f32,
    lead_em: f32,
    side_em: f32,
    button_em: f32,
    margin: f32,
    top: f32,
    bottom: f32,
    gap: f32,
}

impl Setting {
    fn on(size: Size) -> Self {
        if size.width < NARROW {
            Self {
                u: (size.width / 390.0).min(size.height / 780.0).max(0.3),
                one_column: true,
                head_em: 46.0,
                lead_em: 16.5,
                side_em: 17.5,
                button_em: 20.0,
                margin: 24.0,
                top: 50.0,
                bottom: 84.0,
                gap: 16.0,
            }
        } else {
            Self {
                u: (size.width / 1520.0).min(size.height / 950.0).max(0.3),
                one_column: false,
                head_em: 140.0,
                lead_em: 27.0,
                side_em: 31.0,
                button_em: 32.0,
                margin: 80.0,
                top: 92.0,
                bottom: 64.0,
                gap: 40.0,
            }
        }
    }
}

// The sizes the page's two blocks take once laid out: the headline and
// the text under it.
#[derive(Clone, Copy)]
struct Blocks {
    head: MutableState<Size>,
    body: MutableState<Size>,
}

impl Blocks {
    // How far down the headline stands: the blocks centred in the margins.
    fn top(self, setting: Setting, height: f32) -> f32 {
        let u = setting.u;
        let block = self.head.get().height + setting.gap * u + self.body.get().height;
        setting.top * u + ((height - (setting.top + setting.bottom) * u - block) * 0.5).max(0.0)
    }
}

// The section, `top` pixels down the article that `scroll` scrolls; its
// button scrolls the article to `next`.
#[composable(no_skip)]
pub(super) fn PaintSection(size: Size, scroll: ScrollState, (top, next): (f32, f32)) {
    let setting = Setting::on(size);
    let u = setting.u;
    let lamp = rememberLamp();
    let blocks = remember(|| Blocks {
        head: mutableStateOf(Size::default()),
        body: mutableStateOf(Size::default()),
    })
    .with(|blocks| *blocks);
    let button = rememberMutableInteractionSource();
    let (hovered, pressed) = (
        button.collectIsHoveredAsState(),
        button.collectIsPressedAsState(),
    );
    let page = move || {
        let head_top = blocks.top(setting, size.height);
        let head_bottom = head_top + blocks.head.get().height;
        let body_top = head_bottom + setting.gap * u;
        OilPage {
            unit: u,
            head_em: setting.head_em,
            body_em: setting.lead_em,
            head: (head_top / u, head_bottom / u),
            body: (
                body_top / u,
                (body_top + blocks.body.get().height) / u,
                (size.width / u - setting.margin).max(0.0),
            ),
            button: look(&hovered, &pressed),
        }
    };
    let stage = lamp.follow_pointer(Modifier::empty().required_size(size), size);
    Box(
        lit(stage, u, move || {
            let swing = (scroll.value() - top) / size.height.max(1.0);
            lamp.place(swing, size.width / u)
        }),
        BoxSpec::default(),
        move || {
            let first = blocks.top(setting, size.height);
            Box(
                painted(Modifier::empty().required_size(size), page).background(PAGE),
                BoxSpec::default(),
                move || PaintedPage((setting, blocks, first), button, (scroll, next)),
            );
        },
    );
}

fn look(hovered: &State<bool>, pressed: &State<bool>) -> ButtonLook {
    if pressed.get() {
        ButtonLook::Pressed
    } else if hovered.get() {
        ButtonLook::Hovered
    } else {
        ButtonLook::Rest
    }
}

// The page itself, in key colours: the headline, the text and the button,
// set as `setting` says, the headline `first` pixels down. `blocks` takes
// the sizes the headline and the text take. The button scrolls `scroll` to
// `next`.
#[composable(no_skip)]
fn PaintedPage(
    (setting, blocks, first): (Setting, Blocks, f32),
    button: MutableInteractionSource,
    next: (ScrollState, f32),
) {
    let u = setting.u;
    let selection = TextSelectionColors {
        handle_color: SELECTION,
        background_color: SELECTION,
    };
    CompositionLocalProvider(
        [local_text_selection_colors().provides(selection)],
        move || {
            Column(
                Modifier::empty().fill_max_size().padding_each(
                    setting.margin * u,
                    first,
                    setting.margin * u,
                    0.0,
                ),
                ColumnSpec::new()
                    .vertical_arrangement(LinearArrangement::SpacedBy(setting.gap * u)),
                move || {
                    SelectionContainer(
                        Modifier::empty().report_size_state(blocks.head),
                        move || Headline(setting),
                    );
                    if setting.one_column {
                        Column(
                            Modifier::empty()
                                .fill_max_width()
                                .report_size_state(blocks.body),
                            ColumnSpec::new()
                                .vertical_arrangement(LinearArrangement::SpacedBy(14.0 * u)),
                            move || Columns(setting, button, next),
                        );
                    } else {
                        Row(
                            Modifier::empty()
                                .fill_max_width()
                                .report_size_state(blocks.body),
                            RowSpec::new()
                                .horizontal_arrangement(LinearArrangement::SpacedBy(72.0 * u))
                                .vertical_alignment(VerticalAlignment::Top),
                            move || Columns(setting, button, next),
                        );
                    }
                },
            );
        },
    );
}

// The introduction, and beside it, or under it in one column, the claim
// and the button.
#[composable(no_skip)]
fn Columns(setting: Setting, button: MutableInteractionSource, (scroll, next): (ScrollState, f32)) {
    let u = setting.u;
    let share = |weight: f32| {
        if setting.one_column {
            Modifier::empty().fill_max_width()
        } else {
            Modifier::empty().weight(weight)
        }
    };
    SelectionContainer(share(0.57), move || Lead(setting));
    Column(
        share(0.43),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(26.0 * u)),
        move || {
            SelectionContainer(Modifier::empty(), move || Side(setting));
            PaintedButton(setting, button, move || scroll.animate_scroll_to(next));
        },
    );
}

#[composable(no_skip)]
fn Headline(setting: Setting) {
    let words = AnnotatedString::builder()
        .append("Live composables\nin ")
        .push_style(SpanStyle {
            color: Some(ACCENT_INK),
            ..SpanStyle::default()
        })
        .append("Cranpose")
        .pop()
        .to_annotated_string();
    let size = setting.head_em * setting.u;
    let mut style = headline(size, HEAD_INK);
    style.paragraph_style.line_height = TextUnit::Sp(size * HEAD_LINE);
    Text(words, Modifier::empty(), style);
}

#[composable(no_skip)]
fn Lead(setting: Setting) {
    let words = AnnotatedString::builder()
        .append("This article is a ")
        .with_link(LinkAnnotation::Url(REPOSITORY_URL.into()), |builder| {
            underlined(builder, "Cranpose")
        })
        .append(
            " app. Every word on its pages is a live composable: you can select it, follow \
             its link or press its button. Shaders and meshes draw the words as paint, as \
             stamps and as silk. They still work there.",
        )
        .to_annotated_string();
    Paragraph(words, strong_lines(setting.lead_em * setting.u, 1.34));
}

#[composable(no_skip)]
fn Side(setting: Setting) {
    let words = AnnotatedString::builder()
        .append("Its API follows ")
        .with_link(LinkAnnotation::Url(COMPOSE_URL.into()), |builder| {
            underlined(builder, "Jetpack Compose")
        })
        .append(
            ". Here a lamp rakes across thick oil paint. Move the pointer to move the \
             lamp, or select some words to paint over them.",
        )
        .to_annotated_string();
    Paragraph(words, strong_lines(setting.side_em * setting.u, 1.26));
}

fn underlined(builder: Builder, words: &str) -> Builder {
    builder
        .push_style(SpanStyle {
            text_decoration: Some(TextDecoration::UNDERLINE),
            ..SpanStyle::default()
        })
        .append(words)
        .pop()
}

fn strong_lines(size: f32, line: f32) -> TextStyle {
    let mut style = strong(size, BODY_INK);
    style.paragraph_style.line_height = TextUnit::Sp(size * line);
    style
}

#[composable(no_skip)]
fn Paragraph(words: AnnotatedString, style: TextStyle) {
    let uri_handler = local_uri_handler().current();
    LinkedText(words, Modifier::empty(), style, move |url| {
        if let Err(error) = uri_handler.open_uri(url) {
            log::warn!("could not open {url}: {error}");
        }
    });
}

// The button: a slab the painting lays thick orange paint on, which takes
// a fresh coat under the pointer and flattens when pressed.
#[composable(no_skip)]
fn PaintedButton(
    setting: Setting,
    source: MutableInteractionSource,
    on_click: impl Fn() + 'static,
) {
    let (u, em) = (setting.u, setting.button_em);
    Button(
        Modifier::empty()
            .padding_each(10.0 * u, 0.0, 0.0, 0.0)
            .hoverable(source, true)
            .background(SLAB)
            .padding_symmetric(1.15 * em * u, 0.75 * em * u),
        ButtonSpec::new().interaction_source(source),
        on_click,
        move || {
            Text("See it in action", Modifier::empty(), strong(em * u, LABEL));
        },
    );
}
