use std::time::{Duration, Instant};

use cranpose::AppLauncher;
use cranpose_animation::{
    infiniteRepeatable, rememberInfiniteTransition, AnimationSpec, Easing, RepeatMode, StartOffset,
};
use cranpose_ui::{
    composable,
    widgets::{Box, BoxSpec, Column, ColumnSpec, Text},
    Brush, Color, Modifier, Rect, Size, TextStyle,
};

const WARMUP: Duration = Duration::from_secs(3);
const WINDOW: Duration = Duration::from_secs(10);
const CPU_HOOK: &str = "process_cpu_seconds";

fn main() {
    let _ = env_logger::try_init();
    let mode = std::env::var("CRANPOSE_IDLE_MODE").unwrap_or_else(|_| "none".to_string());
    let mode_for_screen = mode.clone();
    AppLauncher::new()
        .with_title("idle burn probe")
        .with_size(800, 600)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(false)
        .with_robot_app_hook(|name, _argument| match name.as_str() {
            CPU_HOOK => cranpose_services::device_info()
                .process_cpu_time()
                .map(|cpu| Some(cpu.as_secs_f64().to_string()))
                .ok_or_else(|| "this platform reports no process CPU time".to_string()),
            _ => Ok(None),
        })
        .with_test_driver(move |robot| {
            let cpu_seconds = || -> f64 {
                match robot.invoke_app_hook(CPU_HOOK, "") {
                    Ok(Some(seconds)) => seconds.parse().unwrap_or(f64::NAN),
                    Ok(None) => f64::NAN,
                    Err(error) => {
                        eprintln!("IDLE-BURN cannot read process CPU time: {error}");
                        std::process::exit(1);
                    }
                }
            };
            std::thread::sleep(WARMUP);
            let before = cpu_seconds();
            let start = Instant::now();
            std::thread::sleep(WINDOW);
            let span = start.elapsed().as_secs_f64();
            let used = cpu_seconds() - before;
            println!(
                "IDLE-BURN mode={mode} cpu={used:.2}s over {span:.1}s = {:.0}% of one core",
                used / span * 100.0
            );
            std::process::exit(0);
        })
        .run(move || {
            IdleScreen(mode_for_screen.clone());
        });
}

/// `none` draws a still screen. `hidden` runs an animation nothing reads,
/// `drawn` shows its value as text, `composition` reads it in composition and
/// moves a line with it, and `draw_behind` reads it inside a draw closure.
#[composable]
fn IdleScreen(mode: String) {
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::new(),
        move || {
            Text(
                "nothing on this screen moves".to_string(),
                Modifier::empty(),
                TextStyle::default(),
            );
            if mode == "none" {
                return;
            }
            let pulse = rememberInfiniteTransition("idle-probe").animateFloat(
                0.0,
                1.0,
                infiniteRepeatable(
                    AnimationSpec::tween(2600, Easing::EaseInOut),
                    RepeatMode::Reverse,
                    StartOffset::default(),
                ),
                "value",
            );
            match mode.as_str() {
                "drawn" => {
                    Text(
                        format!("{:.2}", pulse.get()),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                }
                "composition" => {
                    let value = pulse.get();
                    scan_icon(move || value);
                }
                "draw_behind" => scan_icon(move || pulse.get()),
                _ => {}
            }
        },
    );
}

/// A 64 dp square with a scan line at `position()` of its height, read
/// while drawing.
fn scan_icon(position: impl Fn() -> f32 + 'static) {
    Box(
        Modifier::empty()
            .size(Size::new(64.0, 64.0))
            .draw_behind(move |scope| {
                let y = 64.0 * position();
                scope.draw_rect_at(
                    Rect {
                        x: 0.0,
                        y,
                        width: 64.0,
                        height: 2.0,
                    },
                    Brush::solid(Color(0.2, 0.5, 1.0, 1.0)),
                );
            }),
        BoxSpec::default(),
        || {},
    );
}
