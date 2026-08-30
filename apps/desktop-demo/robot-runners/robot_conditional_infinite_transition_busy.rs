mod robot_launch;

mod robot_exit;

use std::time::{Duration, Instant};

use cranpose::{Robot, RobotScreenshot};
use cranpose_animation::{
    infiniteRepeatable, rememberInfiniteTransition, AnimationSpec, Easing, RepeatMode, StartOffset,
};
use cranpose_core::rememberMutableStateOf;
use cranpose_testing::{
    changed_pixel_count_in_region, find_button_in_semantics, find_text_in_semantics,
};
use cranpose_ui::{
    Brush, Button, ButtonSpec, Color, Column, ColumnSpec, LinearArrangement, Modifier, Rect, Row,
    RowSpec, Text, TextStyle,
};

fn capture(robot: &Robot, label: &str) -> RobotScreenshot {
    robot
        .screenshot()
        .unwrap_or_else(|err| robot_exit::fail(robot, &format!("{label} screenshot failed: {err}")))
}

fn wait_for_text(robot: &Robot, text: &str, timeout: Duration) -> (f32, f32, f32, f32) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(bounds) = find_text_in_semantics(robot, text) {
            return bounds;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    robot_exit::fail(robot, &format!("timed out waiting for text {text:?}"));
}

fn click_button(robot: &Robot, label: &str) {
    let Some((x, y, w, h)) = find_button_in_semantics(robot, label) else {
        robot_exit::fail(robot, &format!("button {label:?} not found"));
    };
    robot
        .click(x + w * 0.5, y + h * 0.5)
        .unwrap_or_else(|err| robot_exit::fail(robot, &format!("click {label:?} failed: {err}")));
}

fn test_app() {
    let busy = rememberMutableStateOf(|| false);

    Column(
        Modifier::empty()
            .fill_max_size()
            .background(Color(0.07, 0.08, 0.10, 1.0))
            .padding(24.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(14.0)),
        move || {
            Text(
                "Conditional Infinite Transition",
                Modifier::empty(),
                TextStyle::default(),
            );
            Button(
                Modifier::empty(),
                ButtonSpec::default(),
                move || {
                    busy.set(true);
                },
                move || {
                    let label = if busy.get() { "Busy" } else { "Start busy" };
                    Text(label, Modifier::empty().padding(8.0), TextStyle::default());
                },
            );

            if busy.get() {
                busy_indicator();
            }
        },
    );
}

fn busy_indicator() {
    let transition = rememberInfiniteTransition("conditional_busy_indicator");
    let phase = transition.animateFloat(
        0.0,
        1.0,
        infiniteRepeatable(
            AnimationSpec::tween(700, Easing::LinearEasing),
            RepeatMode::Restart,
            StartOffset::default(),
        ),
        "conditional_busy_indicator_phase",
    );

    Row(
        Modifier::empty()
            .width(360.0)
            .height(48.0)
            .rounded_corners(8.0)
            .clip_to_bounds()
            .draw_behind(move |scope| {
                let size = scope.size();
                let band_width = 96.0;
                let x = phase.get() * (size.width + band_width) - band_width;
                scope.draw_rect(Brush::solid(Color(0.16, 0.18, 0.22, 1.0)));
                scope.draw_rect_at(
                    Rect {
                        x,
                        y: 0.0,
                        width: band_width,
                        height: size.height,
                    },
                    Brush::solid(Color(0.20, 0.58, 0.90, 1.0)),
                );
            })
            .padding(14.0),
        RowSpec::default(),
        || {
            Text("Busy indicator", Modifier::empty(), TextStyle::default());
        },
    );
}

fn main() {
    env_logger::init();
    println!("=== Conditional Infinite Transition Busy Robot Test ===");

    robot_launch::launch("Conditional Infinite Transition Busy", 800, 500)
        .with_test_driver(|robot| {
            wait_for_text(
                &robot,
                "Conditional Infinite Transition",
                Duration::from_secs(5),
            );
            click_button(&robot, "Start busy");
            let (text_x, text_y, _, _) =
                wait_for_text(&robot, "Busy indicator", Duration::from_secs(5));
            let indicator_region = (text_x - 16.0, text_y - 14.0, 360.0, 52.0);

            robot.pump_frames(2).unwrap_or_else(|err| {
                robot_exit::fail(&robot, &format!("initial pump failed: {err}"))
            });
            let first = capture(&robot, "first busy frame");
            robot.pump_frames(16).unwrap_or_else(|err| {
                robot_exit::fail(&robot, &format!("animation pump failed: {err}"))
            });
            let later = capture(&robot, "later busy frame");

            let changed = changed_pixel_count_in_region(&first, &later, indicator_region, 8);
            if changed < 80 {
                robot_exit::fail(
                    &robot,
                    &format!("busy indicator painted once but did not advance; changed={changed}"),
                );
            }

            println!("PASS: conditional busy animation advanced; changed={changed}");
            robot.exit().ok();
        })
        .run(test_app);
}
