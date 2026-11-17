// The one live paragraph of "How it works", drawn twice: flat on the
// drawing board and bent on the photographic print, two meshes of one
// composable. Select a word on either copy and it is selected on both; the
// last line can be edited on either.
//
// The paragraph's own drawing carries what both copies show, the word a
// pointer points at and the spot under it, and the boxes ruled round its
// lines and words in a green that only the flat copy shows, in blue.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{remember, CompositionLocalProvider};
use cranpose_ui::{
    composable, local_text_selection_colors, space::MeshSurfaces, widgets::SelectionContainer,
    BasicTextField, Brush, Color, CornerRadii, DrawScope, Modifier, Point, PointerEventKind,
    PointerInputScope, Rect, Size, Stroke, Text, TextFieldState, TextSelectionColors,
};

use super::{
    drawing_board::flat_copy,
    figure::{timeline, Figure, Plan},
    photo_print::{bent_copy, print_shows},
    point_back::{PointBack, Pointer},
    sheet_layout::{Block, SheetLayout, BODY, EDIT_LINE, HEADING},
    walk::{ease, Walk},
};

// The ink the paragraph is printed in.
const INK: Color = Color(0.066, 0.082, 0.152, 1.0);
const COBALT: Color = Color(0.12, 0.31, 0.88, 1.0);
// What the flat copy shows in blue and the bent copy leaves out.
const GUIDE_KEY: Color = Color(0.0, 1.0, 0.0, 1.0);
const HIGHLIGHTER: Color = Color(1.0, 0.88, 0.28, 1.0);
const ORANGE: Color = Color(1.0, 0.40, 0.10, 1.0);

// The layout the paragraph's texts take, kept for those that read it
// between compositions: the spot's route and the cord.
pub(super) type LayoutNow = Rc<RefCell<Rc<SheetLayout>>>;

// The paragraph, planned by `plan`, in a scene node of `node`'s size: flat
// on the board and, once the print shows, bent on the print, both posed by
// `pose`.
#[composable(no_skip)]
pub(super) fn Paragraph(
    (node, plan): (Size, Plan),
    (walk, pose): (Walk, impl Fn() -> Figure + Copy + 'static),
    point: PointBack,
    layout_now: LayoutNow,
) {
    let edit = remember(|| TextFieldState::new(EDIT_LINE)).with(|state| *state);
    let layout = Rc::new(SheetLayout::new(plan.u, plan.sheet.height, &edit.text()));
    *layout_now.borrow_mut() = Rc::clone(&layout);
    let marker = TextSelectionColors {
        handle_color: COBALT,
        background_color: HIGHLIGHTER,
    };
    CompositionLocalProvider(
        [local_text_selection_colors().provides(marker)],
        move || {
            let layout = Rc::clone(&layout);
            SelectionContainer(
                Modifier::empty()
                    .required_size(plan.sheet.size())
                    .mesh_surfaces(move || {
                        let progress = walk.progress();
                        let figure = pose();
                        let flat = flat_copy(&figure, node, progress);
                        // An unseen print takes no pointer.
                        let bent = (print_shows(progress) > 0.0)
                            .then(|| bent_copy(&figure, node, progress));
                        MeshSurfaces::new(flat, bent.into_iter().collect())
                    })
                    .background(Color::WHITE)
                    .draw_behind({
                        let layout = Rc::clone(&layout);
                        move |scope| draw_marks(scope, &layout, walk, point)
                    })
                    .pointer_input((), move |scope: PointerInputScope| async move {
                        scope
                            .await_pointer_event_scope(|events| async move {
                                loop {
                                    let event = events.await_pointer_event().await;
                                    let over = !matches!(event.kind, PointerEventKind::Exit);
                                    point.pointer.set(over.then_some(Pointer {
                                        at: event.position,
                                        scroll: walk.scroll.value_non_reactive(),
                                    }));
                                }
                            })
                            .await;
                    }),
                move || {
                    for block in [Block::Heading, Block::Body] {
                        let index = block as usize;
                        let text = if block == Block::Heading {
                            HEADING
                        } else {
                            BODY
                        };
                        Text(
                            text,
                            Modifier::empty()
                                .offset(layout.left, layout.tops[index])
                                .width(layout.width),
                            block.style(plan.u, INK),
                        );
                    }
                    BasicTextField(
                        edit,
                        Modifier::empty().offset(layout.left, layout.tops[Block::Edit as usize]),
                        Block::Edit.style(plan.u, INK),
                    );
                },
            );
        },
    );
}

// What the paragraph draws under its words: the boxes ruled round its
// lines and words as the walk reaches them, the rule under the line that
// can be edited, and at step 4 the pointed word and the spot.
fn draw_marks(scope: &mut dyn DrawScope, layout: &SheetLayout, walk: Walk, point: PointBack) {
    let u = layout.u;
    let progress = walk.progress();
    let pull = super::walk::ease_softly(timeline::PULL_BACK.0, timeline::PULL_BACK.1, progress);
    let thin = 1.0 - 0.2 * pull;
    let key = |alpha: f32| Brush::solid(Color(GUIDE_KEY.0, GUIDE_KEY.1, GUIDE_KEY.2, alpha));
    for (index, line) in layout.lines.iter().enumerate() {
        let drawn = ease(
            0.008 + index as f32 * 0.009,
            0.06 + index as f32 * 0.009,
            progress,
        );
        let pad = 8.0 * u;
        let ruled = Rect {
            x: line.x - pad,
            y: line.y - 0.5 * u,
            width: line.width + 2.0 * pad,
            height: line.height + u,
        };
        outline(scope, ruled, drawn, (key(0.95), 2.4 * u * thin));
        if drawn > 0.0 {
            // A tick out of the top left and the bottom right corner, as a
            // dimension mark.
            let stroke = Stroke::new(1.6 * u * thin);
            let (top, bottom) = (line.y, line.y + line.height);
            let (left, right) = (ruled.x, ruled.x + ruled.width);
            scope.draw_line(
                key(0.8 * drawn),
                Point::new(left - 3.0 * u, top),
                Point::new(left - 13.0 * u, top),
                stroke,
            );
            scope.draw_line(
                key(0.8 * drawn),
                Point::new(right + 3.0 * u, bottom),
                Point::new(right + 13.0 * u, bottom),
                stroke,
            );
        }
    }
    for (index, word) in layout.words.iter().enumerate() {
        let drawn = ease(
            0.085 + index as f32 * 0.0046,
            0.125 + index as f32 * 0.0046,
            progress,
        );
        let ruled = Rect {
            x: word.x - 1.5 * u,
            y: word.y + 4.0 * u,
            width: word.width + 3.0 * u,
            height: word.height - 8.0 * u,
        };
        outline(scope, ruled, drawn, (key(0.62), 1.1 * u * thin));
    }
    scope.draw_rect_at(layout.underline, Brush::solid(COBALT));

    let shows = ease(timeline::CORD.0, timeline::CORD.1, progress);
    if shows <= 0.0 {
        return;
    }
    if let Some(word) = point.word.get() {
        let marked = point.word_show.get() * shows;
        let around = Rect {
            x: word.x - 6.0 * u,
            y: word.y - u,
            width: word.width + 12.0 * u,
            height: word.height + 2.0 * u,
        };
        let corners = CornerRadii::uniform(6.0 * u);
        scope.draw_round_rect_at(
            around,
            Brush::solid(Color(1.0, 0.55, 0.25, 0.42 * marked)),
            corners,
        );
        scope.draw_round_rect_at_stroked(
            around,
            Brush::solid(Color(ORANGE.0, ORANGE.1, ORANGE.2, marked)),
            corners,
            Stroke::new(1.6 * u),
        );
    }
    if let Some(spot) = point.spot.get() {
        let glow = Brush::radial_gradient(
            vec![
                Color(1.0, 0.45, 0.1, 0.45 * shows),
                Color(1.0, 0.45, 0.1, 0.0),
            ],
            spot,
            22.0 * u,
        );
        scope.draw_circle(glow, spot, 22.0 * u);
    }
}

// The first `drawn` share of `rect`'s outline, clockwise from its top left
// corner, in `brush` `width` wide.
fn outline(scope: &mut dyn DrawScope, rect: Rect, drawn: f32, (brush, width): (Brush, f32)) {
    if drawn <= 0.0 {
        return;
    }
    let corners = [
        Point::new(rect.x, rect.y),
        Point::new(rect.x + rect.width, rect.y),
        Point::new(rect.x + rect.width, rect.y + rect.height),
        Point::new(rect.x, rect.y + rect.height),
        Point::new(rect.x, rect.y),
    ];
    let sides = [rect.width, rect.height, rect.width, rect.height];
    let mut left = drawn * 2.0 * (rect.width + rect.height);
    let stroke = Stroke::new(width);
    for (side, length) in sides.into_iter().enumerate() {
        if left <= 0.0 {
            break;
        }
        let share = (left / length.max(1.0e-3)).min(1.0);
        let (from, to) = (corners[side], corners[side + 1]);
        let end = Point::new(
            from.x + (to.x - from.x) * share,
            from.y + (to.y - from.y) * share,
        );
        scope.draw_line(brush.clone(), from, end, stroke);
        left -= length;
    }
}
