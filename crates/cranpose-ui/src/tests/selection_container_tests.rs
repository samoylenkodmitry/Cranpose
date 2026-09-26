use std::sync::Arc;

use cranpose_core::{DefaultScheduler, Runtime};

use super::*;
use crate::text::TextAlign;

/// The test measurer's advance for one character at the default 14 sp.
const CHAR: f32 = 14.0 * 0.6;
/// The test measurer's line height at the default 14 sp.
const LINE: f32 = 14.0;

struct Page {
    runtime: Runtime,
    registrar: SelectionRegistrar,
    keys: Vec<u64>,
}

/// A container holding `texts`, each laid out at its window rectangle.
fn page(texts: &[(&str, Rect)]) -> Page {
    page_styled(texts, TextStyle::default())
}

fn page_styled(texts: &[(&str, Rect)], style: TextStyle) -> Page {
    let runtime = Runtime::new(Arc::new(DefaultScheduler));
    let selection = MutableState::with_runtime(None, runtime.handle());
    let registrar = SelectionRegistrar::new(selection);
    let keys = texts
        .iter()
        .map(|(text, rect)| {
            let key = registrar.subscribe();
            let geometry = Rc::new(SelectableGeometry::default());
            geometry.node_origin.set(Point {
                x: rect.x,
                y: rect.y,
            });
            geometry.set_content_size(Size {
                width: rect.width,
                height: rect.height,
            });
            registrar.update(
                key,
                Rc::new(AnnotatedString::from(*text)),
                style.clone(),
                TextLayoutOptions::default(),
                geometry,
            );
            key
        })
        .collect();
    Page {
        runtime,
        registrar,
        keys,
    }
}

/// Runs `test` where text can be measured.
fn in_app(test: impl FnOnce()) {
    crate::AppContext::new().enter(test);
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// Asserts `actual` are `expected` within the measurer's float rounding.
fn assert_near(actual: &[Rect], expected: &[Rect]) {
    let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
    assert!(
        actual.len() == expected.len()
            && actual.iter().zip(expected).all(|(a, b)| {
                near(a.x, b.x)
                    && near(a.y, b.y)
                    && near(a.width, b.width)
                    && near(a.height, b.height)
            }),
        "{actual:?} is not {expected:?}"
    );
}

fn edge(selectable: u64, offset: usize) -> SelectionEdge {
    SelectionEdge { selectable, offset }
}

fn select(page: &Page, anchor: SelectionEdge, focus: SelectionEdge) {
    page.registrar
        .set_selection(Some(TextSelection { anchor, focus }));
}

#[test]
fn a_point_inside_a_text_selects_to_the_character_under_it() {
    in_app(|| {
        let page = page(&[("hello world", rect(10.0, 20.0, 200.0, LINE))]);

        let at = page.registrar.edge_at(Point {
            x: 10.0 + CHAR * 6.0 + 1.0,
            y: 25.0,
        });

        assert_eq!(at, Some(edge(page.keys[0], 6)));
    });
}

#[test]
fn a_point_outside_the_texts_selects_to_the_nearest_end() {
    in_app(|| {
        let page = page(&[
            ("first", rect(0.0, 0.0, 200.0, LINE)),
            ("second", rect(0.0, 40.0, 200.0, LINE)),
            ("beside", rect(300.0, 40.0, 100.0, LINE)),
        ]);
        let [first, second, beside] = [page.keys[0], page.keys[1], page.keys[2]];

        assert_eq!(
            page.registrar.edge_at(Point { x: 50.0, y: 25.0 }),
            Some(edge(second, 0)),
            "between the rows is the start of the next text"
        );
        assert_eq!(
            page.registrar.edge_at(Point { x: 50.0, y: 90.0 }),
            Some(edge(beside, 6)),
            "past every text is the end of the last in reading order"
        );
        assert_eq!(
            page.registrar.edge_at(Point { x: 280.0, y: 45.0 }),
            Some(edge(beside, 0)),
            "beside texts of one row is the nearest of them"
        );
        assert_eq!(
            page.registrar.edge_at(Point { x: -10.0, y: -10.0 }),
            Some(edge(first, 0))
        );
    });
}

#[test]
fn a_selection_across_texts_reads_them_in_order_whichever_way_it_was_dragged() {
    in_app(|| {
        let page = page(&[
            ("second", rect(0.0, 40.0, 200.0, LINE)),
            ("first line", rect(0.0, 0.0, 200.0, LINE)),
        ]);
        let [second, first] = [page.keys[0], page.keys[1]];

        select(&page, edge(first, 6), edge(second, 3));
        assert_eq!(page.registrar.selected_text(), "line\nsec");

        select(&page, edge(second, 3), edge(first, 6));
        assert_eq!(page.registrar.selected_text(), "line\nsec");
    });
}

#[test]
fn each_text_highlights_the_part_of_the_selection_it_holds() {
    in_app(|| {
        let page = page(&[
            ("first line", rect(0.0, 0.0, 200.0, LINE)),
            ("middle", rect(0.0, 20.0, 200.0, LINE)),
            ("last", rect(0.0, 40.0, 200.0, LINE)),
        ]);
        let [first, middle, last] = [page.keys[0], page.keys[1], page.keys[2]];
        select(&page, edge(first, 6), edge(last, 2));

        assert_near(
            &page.registrar.highlight(first),
            &[rect(CHAR * 6.0, 0.0, CHAR * 4.0, LINE)],
        );
        assert_near(
            &page.registrar.highlight(middle),
            &[rect(0.0, 0.0, CHAR * 6.0, LINE)],
        );
        assert_near(
            &page.registrar.highlight(last),
            &[rect(0.0, 0.0, CHAR * 2.0, LINE)],
        );
    });
}

#[test]
fn a_wrapped_text_highlights_a_rectangle_per_visual_line() {
    in_app(|| {
        let page = page(&[("hello world", rect(0.0, 0.0, CHAR * 6.0, LINE * 2.0))]);
        let key = page.keys[0];
        select(&page, edge(key, 3), edge(key, 9));

        let lines = page.registrar.highlight(key);

        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(lines[0].y, 0.0);
        assert_eq!(lines[1].y, LINE);
        assert_eq!(lines[0].x, CHAR * 3.0);
        assert_eq!(lines[1].x, 0.0);
    });
}

#[test]
fn a_centred_text_is_hit_and_highlighted_where_its_line_is_drawn() {
    in_app(|| {
        let mut style = TextStyle::default();
        style.paragraph_style.text_align = TextAlign::Center;
        let width = CHAR * 10.0;
        let page = page_styled(&[("abcd", rect(0.0, 0.0, width, LINE))], style);
        let key = page.keys[0];
        let left = (width - CHAR * 4.0) * 0.5;

        assert_eq!(
            page.registrar.edge_at(Point {
                x: left + CHAR + 1.0,
                y: 5.0
            }),
            Some(edge(key, 1))
        );
        select(&page, edge(key, 0), edge(key, 4));
        assert_near(
            &page.registrar.highlight(key),
            &[rect(left, 0.0, CHAR * 4.0, LINE)],
        );
    });
}

#[test]
fn a_word_dragged_into_another_text_grows_by_whole_words() {
    in_app(|| {
        let page = page(&[
            ("one two", rect(0.0, 0.0, 200.0, LINE)),
            ("three four", rect(0.0, 20.0, 200.0, LINE)),
        ]);
        let [top, bottom] = [page.keys[0], page.keys[1]];
        let pressed = page
            .registrar
            .unit_at(edge(top, 5), SelectionGranularity::Word)
            .expect("the word under the press");
        assert_eq!(
            pressed.selection(),
            TextSelection {
                anchor: edge(top, 4),
                focus: edge(top, 7),
            }
        );

        assert_eq!(
            page.registrar.dragged(pressed, edge(bottom, 2)),
            Some(TextSelection {
                anchor: edge(top, 4),
                focus: edge(bottom, 5),
            }),
            "forward, the pressed word's start holds and the focus takes the far end of its word"
        );

        let pressed = page
            .registrar
            .unit_at(edge(bottom, 8), SelectionGranularity::Word)
            .expect("the word under the press");
        assert_eq!(
            page.registrar.dragged(pressed, edge(top, 1)),
            Some(TextSelection {
                anchor: edge(bottom, 10),
                focus: edge(top, 0),
            }),
            "backward, the pressed word's end holds and the focus takes the near end of its word"
        );
    });
}

#[test]
fn select_all_runs_from_the_first_text_to_the_end_of_the_last() {
    in_app(|| {
        let page = page(&[
            ("b", rect(0.0, 20.0, 100.0, LINE)),
            ("a", rect(0.0, 0.0, 100.0, LINE)),
        ]);

        page.registrar.select_all();

        assert_eq!(
            page.registrar.selection(),
            Some(TextSelection {
                anchor: edge(page.keys[1], 0),
                focus: edge(page.keys[0], 1),
            })
        );
        assert_eq!(page.registrar.selected_text(), "a\nb");
    });
}

#[test]
fn a_text_that_leaves_takes_a_selection_ending_in_it_along() {
    in_app(|| {
        let page = page(&[
            ("stays", rect(0.0, 0.0, 100.0, LINE)),
            ("leaves", rect(0.0, 20.0, 100.0, LINE)),
        ]);
        select(&page, edge(page.keys[0], 1), edge(page.keys[1], 2));

        page.registrar.unsubscribe(page.keys[1]);

        assert_eq!(page.registrar.selection(), None);
        assert!(page.registrar.highlight(page.keys[0]).is_empty());
    });
}

#[test]
fn the_first_highlighted_line_is_where_the_menu_goes() {
    in_app(|| {
        let page = page(&[("hello world", rect(30.0, 50.0, 200.0, LINE))]);
        select(&page, edge(page.keys[0], 9), edge(page.keys[0], 6));

        assert_near(
            &page
                .registrar
                .first_highlight_in_window()
                .into_iter()
                .collect::<Vec<_>>(),
            &[rect(30.0 + CHAR * 6.0, 50.0, CHAR * 3.0, LINE)],
        );
    });
}

fn gesture(page: &Page) -> Rc<SelectionGesture> {
    let menu_open = MutableState::with_runtime(false, page.runtime.handle());
    Rc::new(SelectionGesture::new(
        page.registrar.clone(),
        page.runtime.handle().frame_clock(),
        menu_open,
    ))
}

fn mouse(kind: crate::PointerEventKind, x: f32, y: f32) -> crate::PointerEvent {
    crate::PointerEvent::new(kind, Point { x, y }, Point { x, y })
}

#[test]
fn a_mouse_drag_selects_characters_and_a_double_click_a_word() {
    in_app(|| {
        let page = page(&[("hello world", rect(0.0, 0.0, 200.0, LINE))]);
        let key = page.keys[0];
        let gesture = gesture(&page);

        gesture.on_down(&mouse(crate::PointerEventKind::Down, CHAR * 1.2, 5.0));
        assert_eq!(
            page.registrar.selection(),
            None,
            "a press alone selects nothing"
        );
        assert!(gesture.on_move(&mouse(crate::PointerEventKind::Move, CHAR * 4.2, 5.0)));
        gesture.on_up();
        assert_eq!(page.registrar.selected_text(), "ell");

        gesture.on_down(&mouse(crate::PointerEventKind::Down, CHAR * 7.2, 5.0));
        gesture.on_up();
        gesture.on_down(&mouse(crate::PointerEventKind::Down, CHAR * 7.2, 5.0));
        gesture.on_up();
        assert_eq!(page.registrar.selected_text(), "world");
        assert_eq!(
            page.registrar
                .selection()
                .map(|selection| selection.anchor.selectable),
            Some(key)
        );
    });
}

#[test]
fn copy_puts_the_selection_on_the_clipboard_and_escape_clears_it() {
    in_app(|| {
        let page = page(&[("hello world", rect(0.0, 0.0, 200.0, LINE))]);
        let gesture = gesture(&page);
        gesture.on_down(&mouse(crate::PointerEventKind::Down, CHAR * 0.2, 5.0));
        gesture.on_move(&mouse(crate::PointerEventKind::Move, CHAR * 5.2, 5.0));
        gesture.on_up();

        let command = crate::Modifiers {
            ctrl: cfg!(not(target_os = "macos")),
            meta: cfg!(target_os = "macos"),
            ..crate::Modifiers::NONE
        };
        let copy = crate::KeyEvent::key_down_with_modifiers(crate::KeyCode::C, "c", command);
        assert!(gesture.on_key(&copy));
        assert_eq!(
            crate::clipboard_session::clipboard_read_text().as_deref(),
            Some("hello")
        );

        let escape = crate::KeyEvent::key_down(crate::KeyCode::Escape, "");
        assert!(gesture.on_key(&escape));
        assert_eq!(page.registrar.selection(), None);
        assert!(!gesture.on_key(&copy), "nothing is left to copy");
    });
}

#[test]
fn a_press_in_another_container_clears_this_one() {
    in_app(|| {
        let first = page(&[("first", rect(0.0, 0.0, 200.0, LINE))]);
        let second = page(&[("second", rect(0.0, 0.0, 200.0, LINE))]);
        let (first_gesture, second_gesture) = (gesture(&first), gesture(&second));
        first_gesture.on_down(&mouse(crate::PointerEventKind::Down, 1.0, 5.0));
        first_gesture.on_move(&mouse(crate::PointerEventKind::Move, CHAR * 3.2, 5.0));
        first_gesture.on_up();
        assert!(first.registrar.selection().is_some());

        second_gesture.on_down(&mouse(crate::PointerEventKind::Down, 1.0, 5.0));

        assert_eq!(first.registrar.selection(), None);
    });
}
