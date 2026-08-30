mod robot_launch;

use std::{cmp::Ordering, time::Duration};

use cranpose_foundation::{
    lazy::{rememberLazyListState, LazyListScope},
    SemanticsConfiguration,
};
use cranpose_testing::find_element_by_text_exact;
use cranpose_ui::{
    composable,
    widgets::{LazyColumn, LazyColumnSpec},
    Color, Column, ColumnSpec, LinearArrangement, Modifier, Size, Spacer, Text, TextStyle,
};

fn collect_visible_items(robot: &cranpose::Robot) -> Vec<(usize, f32)> {
    let Ok(semantics) = robot.get_semantics() else {
        return Vec::new();
    };

    let viewport = find_element_by_text_exact(&semantics, "LazyListViewport").map(|elem| {
        (
            elem.bounds.x,
            elem.bounds.y,
            elem.bounds.width,
            elem.bounds.height,
        )
    });
    let Some((vx, vy, vw, vh)) = viewport else {
        return Vec::new();
    };
    let vx2 = vx + vw;
    let vy2 = vy + vh;

    fn visit(
        elem: &cranpose::SemanticElement,
        vx: f32,
        vy: f32,
        vx2: f32,
        vy2: f32,
        out: &mut Vec<(usize, f32)>,
    ) {
        let ex2 = elem.bounds.x + elem.bounds.width;
        let ey2 = elem.bounds.y + elem.bounds.height;
        let overlaps = elem.bounds.x < vx2 && ex2 > vx && elem.bounds.y < vy2 && ey2 > vy;
        if overlaps {
            if let Some(text) = elem.text.as_deref() {
                if let Some(index) = text
                    .strip_prefix("Item ")
                    .and_then(|value| value.parse().ok())
                {
                    out.push((index, elem.bounds.y));
                }
            }
            for child in &elem.children {
                visit(child, vx, vy, vx2, vy2, out);
            }
        }
    }

    let mut items = Vec::new();
    for elem in &semantics {
        visit(elem, vx, vy, vx2, vy2, &mut items);
    }
    items.sort_by(|left, right| left.1.partial_cmp(&right.1).unwrap_or(Ordering::Equal));
    items.dedup_by_key(|entry| entry.0);
    items
}

#[allow(non_snake_case)]
#[composable]
fn VariableHeightWheelReproScreen() {
    let list_state = rememberLazyListState();

    Column(
        Modifier::empty()
            .fill_max_size()
            .padding(16.0)
            .background(Color(0.08, 0.10, 0.14, 1.0)),
        ColumnSpec::default(),
        move || {
            Text(
                "Wheel Scroll Repro",
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );

            Spacer(Size {
                width: 0.0,
                height: 12.0,
            });

            LazyColumn(
                Modifier::empty()
                    .semantics(|config: &mut SemanticsConfiguration| {
                        config.content_description = Some("LazyListViewport".to_string());
                    })
                    .fill_max_width()
                    .weight(1.0),
                list_state,
                LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                move |scope| {
                    scope.items(200, |index| {
                            let repeat_count = match index % 8 {
                                0 => 1,
                                1 => 2,
                                2 => 12,
                                3 => 3,
                                4 => 5,
                                5 => 9,
                                6 => 2,
                                _ => 6,
                            };
                            let body = (0..repeat_count)
                                .map(|line| {
                                    format!(
                                        "Line {} for item {} with enough text to wrap across the viewport width.",
                                        line + 1,
                                        index
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join(" ");
                            Column(
                                Modifier::empty()
                                    .fill_max_width()
                                    .background(Color(0.18, 0.22, 0.28, 1.0))
                                    .padding(8.0),
                                ColumnSpec::default(),
                                move || {
                                    Text(
                                        format!("Item {}", index),
                                        Modifier::empty(),
                                        TextStyle::default(),
                                    );
                                    Spacer(Size {
                                        width: 0.0,
                                        height: 4.0,
                                    });
                                    Text(body.clone(), Modifier::empty(), TextStyle::default());
                                },
                            );
                        },
                    );
                },
            );
        },
    );
}

fn main() {
    env_logger::init();
    println!("=== Lazy Wheel Backtrack Test ===");

    robot_launch::launch("Lazy Wheel Backtrack", 640, 800)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(600));
            let _ = robot.wait_for_idle();

            let viewport_bounds = {
                let semantics = robot.get_semantics().expect("semantics");
                let viewport = find_element_by_text_exact(&semantics, "LazyListViewport")
                    .expect("LazyListViewport not found");
                (
                    viewport.bounds.x,
                    viewport.bounds.y,
                    viewport.bounds.width,
                    viewport.bounds.height,
                )
            };
            let center_x = viewport_bounds.0 + viewport_bounds.2 * 0.5;
            let center_y = viewport_bounds.1 + viewport_bounds.3 * 0.5;
            robot
                .mouse_move(center_x, center_y)
                .expect("move to viewport");

            let initial_items = collect_visible_items(&robot);
            println!(
                "initial visible: {:?}",
                initial_items
                    .iter()
                    .map(|(index, _)| *index)
                    .collect::<Vec<_>>()
            );
            assert!(!initial_items.is_empty(), "expected initial visible items");

            let mut down_top_history = Vec::new();
            for step in 0..10 {
                robot
                    .mouse_scroll(0.0, -120.0)
                    .expect("forward wheel scroll should succeed");
                std::thread::sleep(Duration::from_millis(80));
                let _ = robot.wait_for_idle();
                let visible = collect_visible_items(&robot);
                let top = visible
                    .first()
                    .map(|(index, _)| *index)
                    .expect("top item after down");
                down_top_history.push(top);
                println!(
                    "down step {} top={} visible={:?}",
                    step,
                    top,
                    visible.iter().map(|(index, _)| *index).collect::<Vec<_>>()
                );
            }

            let mut previous_top = *down_top_history.last().expect("down history");
            for step in 0..8 {
                robot
                    .mouse_scroll(0.0, 120.0)
                    .expect("reverse wheel scroll should succeed");
                std::thread::sleep(Duration::from_millis(80));
                let _ = robot.wait_for_idle();
                let visible = collect_visible_items(&robot);
                let top = visible
                    .first()
                    .map(|(index, _)| *index)
                    .expect("top item after up");
                println!(
                    "up step {} top={} visible={:?}",
                    step,
                    top,
                    visible.iter().map(|(index, _)| *index).collect::<Vec<_>>()
                );
                assert!(
                    top <= previous_top,
                    "reverse wheel scroll backtracked from top item {previous_top} to {top}"
                );
                previous_top = top;
            }

            robot.exit().expect("exit");
        })
        .run(|| {
            VariableHeightWheelReproScreen();
        });
}
