use cranpose::AppLauncher;
use cranpose_animation::{
    infiniteRepeatable, rememberInfiniteTransition, AnimationSpec, Easing, RepeatMode, StartOffset,
};
use cranpose_ui::widgets::{Column, ColumnSpec, Text};
use cranpose_ui::{composable, Modifier, TextStyle};
use std::time::{Duration, Instant};

const WARMUP: Duration = Duration::from_secs(3);
const WINDOW: Duration = Duration::from_secs(10);

fn cpu_seconds() -> f64 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let fields: Vec<&str> = stat.split_whitespace().collect();
    let ticks: f64 = fields
        .get(13)
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0)
        + fields
            .get(14)
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0);
    ticks / 100.0
}

fn main() {
    let _ = env_logger::try_init();
    let mode = std::env::var("CRANPOSE_IDLE_MODE").unwrap_or_else(|_| "none".to_string());
    let mode_for_screen = mode.clone();
    AppLauncher::new()
        .with_title("idle burn probe")
        .with_size(800, 600)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(false)
        .with_test_driver(move |_robot| {
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

#[composable]
#[allow(non_snake_case)]
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
            if mode == "hidden" || mode == "drawn" {
                let pulse = rememberInfiniteTransition("idle-probe").animateFloat(
                    0.0,
                    1.0,
                    infiniteRepeatable(
                        AnimationSpec::tween(900, Easing::EaseInOut),
                        RepeatMode::Reverse,
                        StartOffset::default(),
                    ),
                    "value",
                );
                if mode == "drawn" {
                    Text(
                        format!("{:.2}", pulse.get()),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                }
            }
        },
    );
}
