// The melt section, "So words can melt". Its stage stays in view while the
// scroll walks the section, and the words on it turn to molten chocolate:
// they soften, swell, sag and drip, and a pointer warms the ones under it.
// The words lie on a sheet whose mesh the sag bends, so they stay live: a
// pointer finds the letter it shows over, however far the letter has slid,
// and a selection shows as mint icing.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{key, rememberKeyed, CompositionLocalProvider};
use cranpose_ui::{
    composable, local_text_selection_colors,
    space::{Mat4, MeshSurface},
    text::{AnnotatedString, TextLayoutOptions},
    widgets::SelectionContainer,
    Box, BoxSpec, Color, Column, ColumnSpec, LinearArrangement, Modifier, Point, ScrollState, Size,
    Text, TextSelectionColors, TextStyle,
};

use super::{
    chocolate::{chocolate_layer, poured, Pour},
    drips::{plant, split, Drip, DripPlan, TypeBlock},
    hover::rememberHover,
    keys::{COPY_INK, HEAD_INK, SELECTION_KEY},
    lettering::display,
    melting::Melting,
    plaster::PlasterWall,
    puddle::Puddle,
    roots::Ink,
    sag::Sheet,
    stage::{SceneStage, STAGE_LIGHT},
    walk::{unit, Walk},
};

pub(super) const MELT_PAGES: f32 = 2.5;
const HEADLINE: &str = "So words\ncan melt";
const PARAGRAPH: &str = "As you scroll, this paragraph heats up. It softens, sags and runs. \
                         Cranpose still lays out every letter, so you can select the drips. \
                         Copy them and they paste straight.";
// In front of the wall, so the shadow on the wall lies behind the
// chocolate.
const SHEET_DEPTH: f32 = -2.0;
const MINT: Color = Color(0.21, 0.67, 0.56, 1.0);
// Against the type size, in Caprasimo.
const CAP_HEIGHT: f32 = 0.7;

#[composable(no_skip)]
pub(super) fn MeltSection(size: Size, scroll: ScrollState, top: f32, moving: bool) {
    let walk = Walk::new(scroll, top, size.height, MELT_PAGES);
    Box(
        Modifier::empty()
            .required_size(Size::new(size.width, size.height * MELT_PAGES))
            .test_tag("melt"),
        BoxSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .required_size(size)
                    .graphics_layer_block(move |layer| layer.translation_y = walk.hold()),
                BoxSpec::default(),
                move || MeltStage(size, walk, moving),
            );
        },
    );
}

#[composable(no_skip)]
fn MeltStage(size: Size, walk: Walk, moving: bool) {
    let u = unit(size);
    let lettering = rememberKeyed((size.width.to_bits(), size.height.to_bits()), |_| {
        Rc::new(Lettering::new(size, u))
    });
    let hover = rememberHover(moving);
    let melting = Melting {
        walk,
        hover,
        size,
        u,
        split: lettering.split,
    };
    let density = cranpose_ui::density().density();
    Box(
        hover.follow(Modifier::empty().required_size(size)),
        BoxSpec::default(),
        move || {
            let lettering = Rc::clone(&lettering);
            SceneStage(size, Some(STAGE_LIGHT), move || {
                PlasterWall(size, u);
                MeltSheet(Rc::clone(&lettering), melting, density);
                Puddle(
                    melting,
                    (Rc::clone(&lettering.landings), SHEET_DEPTH - 0.5),
                    density,
                );
            });
        },
    );
}

#[composable(no_skip)]
fn MeltSheet(lettering: Rc<Lettering>, melting: Melting, density: f32) {
    let size = melting.size;
    let sheet = rememberKeyed((size.width.to_bits(), size.height.to_bits()), |_| {
        Rc::new(RefCell::new(Sheet::new(size, SHEET_DEPTH)))
    });
    let pour = Pour {
        size,
        offset: Point::new(0.0, 0.0),
        head_scale: lettering.head_scale,
        copy_scale: lettering.copy_scale,
    };
    Box(
        Modifier::empty()
            .required_size(size)
            .mesh_surface(move || MeshSurface {
                mesh: sheet.borrow_mut().mesh(melting.sag()),
                transform: Mat4::IDENTITY,
                material: poured(),
                casts_shadow: false,
                solid: false,
            }),
        BoxSpec::default(),
        move || {
            let lettering = Rc::clone(&lettering);
            Box(
                Modifier::empty()
                    .fill_max_size()
                    .graphics_layer(move || chocolate_layer(pour, melting, density)),
                BoxSpec::default(),
                move || {
                    for (index, plan) in lettering.drips.iter().copied().enumerate() {
                        key(index, move || Drip(plan, melting));
                    }
                    MeltWords(Rc::clone(&lettering));
                },
            );
        },
    );
}

#[composable(no_skip)]
fn MeltWords(lettering: Rc<Lettering>) {
    let icing = TextSelectionColors {
        handle_color: MINT,
        background_color: SELECTION_KEY,
    };
    CompositionLocalProvider([local_text_selection_colors().provides(icing)], move || {
        let lettering = Rc::clone(&lettering);
        SelectionContainer(Modifier::empty().fill_max_size(), move || {
            let lettering = Rc::clone(&lettering);
            Column(
                Modifier::empty().padding_each(
                    lettering.head_at.x,
                    lettering.head_at.y,
                    lettering.head_at.x,
                    0.0,
                ),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(lettering.gap)),
                move || {
                    Text(HEADLINE, Modifier::empty(), lettering.head_style.clone());
                    Text(
                        PARAGRAPH,
                        Modifier::empty().width(lettering.copy_width),
                        lettering.copy_style.clone(),
                    );
                },
            );
        });
    });
}

struct Lettering {
    head_style: TextStyle,
    copy_style: TextStyle,
    head_at: Point,
    gap: f32,
    copy_width: f32,
    split: f32,
    head_scale: f32,
    copy_scale: f32,
    drips: Vec<DripPlan>,
    landings: Rc<[f32]>,
}

impl Lettering {
    fn new(size: Size, u: f32) -> Self {
        let (head_size, copy_size) = (258.0 * u, 43.0 * u);
        let head_style = display(head_size, 0.83, HEAD_INK);
        let copy_style = display(copy_size, 1.28, COPY_INK);
        let side = (84.0 * u).max(16.0);
        let head_at = Point::new(side, 30.0 * u);
        let gap = 40.0 * u;
        let width = size.width - side * 2.0;
        let copy_width = (26.0 * copy_size).min(width);
        let layout = |text: &str, style: &TextStyle, width: f32| {
            cranpose_ui::text::prepare_text_layout(
                &AnnotatedString::from(text),
                style,
                TextLayoutOptions::default(),
                Some(width),
            )
        };
        let head_layout = layout(HEADLINE, &head_style, width);
        let copy_layout = layout(PARAGRAPH, &copy_style, copy_width);
        let copy_at = Point::new(side, head_at.y + head_layout.layout.height() + gap);
        let cell = (size.width / 1100.0).max(1.0);
        let mut ink = Ink::new(size.width, size.height, cell);
        ink.fill(&head_layout.layout, head_at);
        ink.fill(&copy_layout.layout, copy_at);
        let head = TypeBlock::new(&head_layout.layout, head_at, head_size, CAP_HEIGHT);
        let copy = TypeBlock::new(&copy_layout.layout, copy_at, copy_size, CAP_HEIGHT);
        let split = split(&head, &copy);
        let drips = plant((&head, &copy), &ink, (u, split, size.height));
        let landings = drips.iter().filter_map(DripPlan::landing).collect();
        Self {
            head_style,
            copy_style,
            head_at,
            gap,
            copy_width,
            split,
            head_scale: (head_size / (226.0 * u)).clamp(0.2, 1.2),
            copy_scale: (copy_size / (41.0 * u)).clamp(0.4, 1.2),
            drips,
            landings,
        }
    }
}
