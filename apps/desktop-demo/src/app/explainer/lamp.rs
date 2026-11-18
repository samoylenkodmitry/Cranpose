// The lamp section, "Words can light a room". A dark room seen from the
// front: its back wall is a plate with the live words cut through it, and
// one lamp behind it lights the room through them. The words glow, their
// light spills over the plaster and streams through the dusty air, and the
// lamp throws them onto the floor. A selection knocks an opening out of
// the plate and glazes it with coloured glass; once, the words the
// paragraph asks for select themselves. The lamp swings as the scroll walks
// the section and leans toward the pointer.

use std::{cell::Cell, rc::Rc, time::Duration};

use cranpose_core::{
    delay, derivedStateOf, remember, rememberKeyed, CompositionLocalProvider, LaunchedEffect,
};
use cranpose_ui::{
    composable, local_text_selection_colors,
    space::{Multisample, Scene3d},
    text::{AnnotatedString, TextAlign, TextLayoutOptions},
    widgets::{local_selection_registrar, SelectionContainer},
    Box, BoxSpec, Color, Column, ColumnSpec, HorizontalAlignment, LinearArrangement, Modifier,
    ScrollState, Size, Text, TextRange, TextSelectionColors, TextStyle,
};

use super::{
    hover::rememberHover,
    keys::{COPY_INK, HEAD_INK, SELECTION_KEY},
    lettering::{display, strong},
    plate::{Cuts, Plate},
    room::Room,
    skirting::Skirting,
    stage::StageIn,
    walk::{unit, Walk},
};

pub(super) const LAMP_PAGES: f32 = 1.9;
const HEADLINE: &str = "Words can light a room";
const PARAGRAPH: &str = "Each letter is a hole in the wall with a lamp behind it. Cranpose works \
                         the light out from the letters, so when they change, it changes. Select \
                         some words to color the room.";
// The words the paragraph asks a reader to select.
const ASKED: &str = "Select some words";
// How long the stage holds in view before the asked words select
// themselves, and how many milliseconds they take to.
const ASK_AFTER: Duration = Duration::from_millis(1200);
const ASK_MILLIS: usize = 900;
const HANDLE: Color = Color(1.0, 0.8, 0.4, 1.0);

#[composable(no_skip)]
pub(super) fn LampSection(size: Size, scroll: ScrollState, top: f32, moving: bool) {
    let walk = Walk::new(scroll, top, size.height, LAMP_PAGES);
    Box(
        Modifier::empty()
            .required_size(Size::new(size.width, size.height * LAMP_PAGES))
            .test_tag("lamp"),
        BoxSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .required_size(size)
                    .graphics_layer_block(move |layer| layer.translation_y = walk.hold()),
                BoxSpec::default(),
                move || LampStage(size, walk, moving),
            );
        },
    );
}

#[composable(no_skip)]
fn LampStage(size: Size, walk: Walk, moving: bool) {
    let u = unit(size);
    let hover = rememberHover(moving);
    let room = Room {
        walk,
        hover,
        size,
        u,
    };
    let lettering = rememberKeyed((size.width.to_bits(), size.height.to_bits()), |_| {
        Rc::new(LampLettering::new(room))
    });
    let density = cranpose_ui::density().density();
    Box(
        hover.follow(Modifier::empty().required_size(size)),
        BoxSpec::default(),
        move || {
            let lettering = Rc::clone(&lettering);
            StageIn(
                size,
                move || Scene3d {
                    multisample: Multisample::X4,
                    ..Scene3d::new(room.camera())
                },
                move || {
                    let lettering = Rc::clone(&lettering);
                    Plate(room, (lettering.cuts, density), move || {
                        LampWords(Rc::clone(&lettering), walk);
                    });
                    Skirting(room);
                },
            );
        },
    );
}

#[composable(no_skip)]
fn LampWords(lettering: Rc<LampLettering>, walk: Walk) {
    let glass = TextSelectionColors {
        handle_color: HANDLE,
        background_color: SELECTION_KEY,
    };
    CompositionLocalProvider([local_text_selection_colors().provides(glass)], move || {
        let lettering = Rc::clone(&lettering);
        SelectionContainer(Modifier::empty().fill_max_size(), move || {
            SelectAsked(walk);
            let lettering = Rc::clone(&lettering);
            Column(
                Modifier::empty().fill_max_size().padding_each(
                    lettering.side,
                    lettering.top,
                    lettering.side,
                    lettering.bottom,
                ),
                ColumnSpec::new()
                    .vertical_arrangement(LinearArrangement::SpacedByAligned {
                        spacing: lettering.gap,
                        bias: 0.0,
                    })
                    .horizontal_alignment(HorizontalAlignment::CenterHorizontally),
                move || {
                    Text(HEADLINE, Modifier::empty(), lettering.head_style.clone());
                    Text(
                        PARAGRAPH,
                        Modifier::empty().width_in(0.0, lettering.copy_width),
                        lettering.copy_style.clone(),
                    );
                },
            );
        });
    });
}

// Once, for a reader who has selected nothing in the lamp's words: when the
// stage has held in view for a while, the words the paragraph asks for
// select themselves letter by letter, so the coloured light shows by
// itself. A selection the reader makes stops it.
#[composable(no_skip)]
fn SelectAsked(walk: Walk) {
    let Some(registrar) = local_selection_registrar().current() else {
        return;
    };
    let pinned =
        derivedStateOf(move || (0.0..=walk.travel).contains(&(walk.scroll.value() - walk.top)))
            .get();
    let done = remember(|| Rc::new(Cell::new(false))).with(Rc::clone);
    LaunchedEffect(pinned, async move |scope| {
        if !pinned || done.get() {
            return;
        }
        delay(ASK_AFTER).await;
        done.set(true);
        let Some(start) = registrar.text().find(ASKED) else {
            return;
        };
        let began = scope.with_frame_nanos(|now| now).await;
        let mut written = None;
        loop {
            let now = scope.with_frame_nanos(|now| now).await;
            if registrar.selection() != written {
                return;
            }
            let millis = usize::try_from(now.saturating_sub(began) / 1_000_000)
                .map_or(ASK_MILLIS, |millis| millis.min(ASK_MILLIS));
            let letters = (ASKED.len() * millis).div_ceil(ASK_MILLIS).max(1);
            registrar.select(TextRange::new(start, start + letters));
            written = registrar.selection();
            if letters == ASKED.len() {
                return;
            }
        }
    });
}

struct LampLettering {
    head_style: TextStyle,
    copy_style: TextStyle,
    side: f32,
    top: f32,
    bottom: f32,
    gap: f32,
    copy_width: f32,
    cuts: Cuts,
}

impl LampLettering {
    fn new(room: Room) -> Self {
        let (u, size) = (room.u, room.size);
        let head_size = 108.0 * u;
        let head_style = display(head_size, 1.0, HEAD_INK);
        let mut copy_style = strong(40.0 * u, COPY_INK);
        copy_style.paragraph_style.line_height = cranpose_ui::text::TextUnit::Sp(40.0 * u * 1.4);
        copy_style.paragraph_style.text_align = TextAlign::Center;
        let side = (100.0 * u).max(16.0);
        let top = 34.0 * u;
        // The words sit in the upper part of the wall, clear of the floor.
        let bottom = size.height * (1.0 - 0.63);
        let gap = 46.0 * u;
        let copy_width = (24.5 * 40.0 * u).min(size.width - side * 2.0);
        let measure = |text: &str, style: &TextStyle, width: f32| {
            cranpose_ui::text::prepare_text_layout(
                &AnnotatedString::from(text),
                style,
                TextLayoutOptions::default(),
                Some(width),
            )
            .layout
            .height()
        };
        let head_height = measure(HEADLINE, &head_style, size.width - side * 2.0);
        let copy_height = measure(PARAGRAPH, &copy_style, copy_width);
        let free = size.height - bottom - top;
        let block = head_height + gap + copy_height;
        let head_bottom = top + (free - block).max(0.0) * 0.5 + head_height;
        Self {
            head_style,
            copy_style,
            side,
            top,
            bottom,
            gap,
            copy_width,
            cuts: Cuts {
                head_size,
                head_bottom,
            },
        }
    }
}
