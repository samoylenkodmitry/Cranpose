mod robot_launch;

use std::time::Duration;

use cranpose_core::remember;
use cranpose_foundation::lazy::{rememberLazyListState, LazyListScope};
use cranpose_testing::{find_button_in_semantics, find_text_in_semantics};
use cranpose_ui::{
    composable,
    widgets::{
        Box, BoxSpec, Button, ButtonSpec, Column, ColumnSpec, LazyColumn, LazyColumnSpec, Row,
        RowSpec, Spacer, Text,
    },
    Modifier, ScrollState, Size, TextStyle,
};

#[composable]
fn overscroll_reproduction() {
    let state = remember(|| ScrollState::new(0.0)).with(|state| *state);
    let lazy_state = rememberLazyListState();
    let outer_state = remember(|| ScrollState::new(0.0)).with(|state| *state);
    let inner_state = remember(|| ScrollState::new(0.0)).with(|state| *state);
    Row(
        Modifier::empty().fill_max_size().height(600.0),
        RowSpec::default(),
        move || {
            Box(
                Modifier::empty().fill_max_height().width(200.0),
                BoxSpec::default(),
                move || {
                    Column(
                        Modifier::empty()
                            .fill_max_size()
                            .vertical_scroll(state, false),
                        ColumnSpec::default(),
                        || {
                            Text("Overscroll Marker", Modifier::empty(), TextStyle::default());
                            Spacer(Size {
                                width: 0.0,
                                height: 1_200.0,
                            });
                        },
                    );
                },
            );
            LazyColumn(
                Modifier::empty().fill_max_height().width(200.0),
                lazy_state,
                LazyColumnSpec::default(),
                |scope| {
                    scope.items(40, |index| {
                        if index == 0 {
                            Text(
                                "Lazy Overscroll Marker",
                                Modifier::empty(),
                                TextStyle::default(),
                            );
                        } else {
                            Spacer(Size {
                                width: 0.0,
                                height: 48.0,
                            });
                        }
                    });
                },
            );
            Box(
                Modifier::empty().fill_max_height().width(200.0),
                BoxSpec::default(),
                move || {
                    Column(
                        Modifier::empty()
                            .fill_max_size()
                            .vertical_scroll(outer_state, false),
                        ColumnSpec::default(),
                        move || {
                            Text(
                                "Nested Outer Marker",
                                Modifier::empty(),
                                TextStyle::default(),
                            );
                            Button(
                                Modifier::empty().height(48.0),
                                ButtonSpec::default(),
                                move || inner_state.scroll_to(inner_state.max_value()),
                                || {
                                    Text("Pin Inner", Modifier::empty(), TextStyle::default());
                                },
                            );
                            Column(
                                Modifier::empty()
                                    .fill_max_width()
                                    .height(300.0)
                                    .vertical_scroll(inner_state, false),
                                ColumnSpec::default(),
                                || {
                                    Spacer(Size {
                                        width: 0.0,
                                        height: 700.0,
                                    });
                                    Text(
                                        "Nested Inner Marker",
                                        Modifier::empty(),
                                        TextStyle::default(),
                                    );
                                },
                            );
                            Spacer(Size {
                                width: 0.0,
                                height: 800.0,
                            });
                        },
                    );
                },
            );
        },
    );
}

fn main() {
    robot_launch::launch("Overscroll Bounce Reproduction", 600, 600).with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let (_, initial_y, width, height) =
                find_text_in_semantics(&robot, "Overscroll Marker").expect("marker missing");
            let x = width * 0.5;
            let y = initial_y + height * 0.5;
            robot.mouse_move(x, y).expect("move to marker");
            robot.mouse_down().expect("press marker");
            robot.mouse_move(x, y + 140.0).expect("drag past top edge");
            std::thread::sleep(Duration::from_millis(300));

            let (_, stretched_y, _, _) =
                find_text_in_semantics(&robot, "Overscroll Marker").expect("marker missing");
            assert!(
                stretched_y > initial_y + 20.0,
                "top-edge drag must stretch content, initial_y={initial_y}, stretched_y={stretched_y}"
            );

            robot.mouse_up().expect("release marker");
            std::thread::sleep(Duration::from_millis(700));
            let (_, settled_y, _, _) =
                find_text_in_semantics(&robot, "Overscroll Marker").expect("marker missing");
            assert!(
                (settled_y - initial_y).abs() < 4.0,
                "overscroll must spring back to the edge, initial_y={initial_y}, settled_y={settled_y}"
            );

            let (lazy_initial_x, lazy_initial_y, lazy_width, _lazy_height) =
                find_text_in_semantics(&robot, "Lazy Overscroll Marker")
                    .expect("lazy marker missing");
            let lazy_x = lazy_initial_x + lazy_width * 0.5;
            let lazy_y = lazy_initial_y + 150.0;
            robot.mouse_move(lazy_x, lazy_y).expect("move to lazy marker");
            robot.mouse_down().expect("press lazy marker");
            robot
                .mouse_move(lazy_x, lazy_y + 140.0)
                .expect("drag lazy list past top edge");
            std::thread::sleep(Duration::from_millis(300));
            let _ = robot.wait_for_idle();

            let (_, lazy_stretched_y, _, _) =
                find_text_in_semantics(&robot, "Lazy Overscroll Marker")
                    .expect("lazy marker missing");
            assert!(
                lazy_stretched_y > lazy_initial_y + 20.0,
                "lazy top-edge drag must stretch content, initial_y={lazy_initial_y}, stretched_y={lazy_stretched_y}"
            );

            robot.mouse_up().expect("release lazy marker");
            std::thread::sleep(Duration::from_millis(700));
            let (_, lazy_settled_y, _, _) =
                find_text_in_semantics(&robot, "Lazy Overscroll Marker")
                    .expect("lazy marker missing");
            assert!(
                (lazy_settled_y - lazy_initial_y).abs() < 4.0,
                "lazy overscroll must spring back to the edge, initial_y={lazy_initial_y}, settled_y={lazy_settled_y}"
            );

            let (button_x, button_y, button_width, button_height) =
                find_button_in_semantics(&robot, "Pin Inner").expect("pin button missing");
            robot
                .click(
                    button_x + button_width * 0.5,
                    button_y + button_height * 0.5,
                )
                .expect("pin inner list");
            std::thread::sleep(Duration::from_millis(300));
            let (_, outer_initial_y, _, _) =
                find_text_in_semantics(&robot, "Nested Outer Marker")
                    .expect("outer marker missing");
            let (inner_x, inner_y, inner_width, inner_height) =
                find_text_in_semantics(&robot, "Nested Inner Marker")
                    .expect("inner marker missing");
            let drag_x = inner_x + inner_width * 0.5;
            let drag_y = inner_y + inner_height * 0.5;
            robot.mouse_move(drag_x, drag_y).expect("move to inner list");
            robot.mouse_down().expect("press inner list");
            robot
                .mouse_move(drag_x, drag_y - 120.0)
                .expect("drag exhausted inner list");
            robot.mouse_up().expect("release inner list");
            std::thread::sleep(Duration::from_millis(300));
            let (_, outer_scrolled_y, _, _) =
                find_text_in_semantics(&robot, "Nested Outer Marker")
                    .expect("outer marker missing after drag");
            assert!(
                outer_scrolled_y < outer_initial_y - 40.0,
                "an exhausted nested scrollable must yield to its parent, initial_y={outer_initial_y}, scrolled_y={outer_scrolled_y}"
            );
            robot.exit().expect("exit");
        })
        .run(overscroll_reproduction);
}
