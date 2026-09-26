use crate::{robot_exit, robot_liquid_stage, robot_shot};

use std::{path::PathBuf, process::ExitCode, sync::atomic::AtomicBool, time::Duration};

use cranpose::{
    liquid::prelude::*,
    widgets::{Box as CBox, BoxSpec},
    AppLauncher, GraphicsLayer, Modifier, RobotScreenshot, Size,
};
use robot_liquid_stage::LiquidStripedStage;

const WINDOW_WIDTH: u32 = 800;
const WINDOW_HEIGHT: u32 = 400;
const TILE_WIDTH: f32 = 208.0;
const TILE_HEIGHT: f32 = 176.0;
const TILE_TOP: f32 = 112.0;
const TILE_RADIUS: f32 = 24.0;
const MERGED_LEFT: f32 = 96.0;
/// The wrapped tile's offset from the merged one: a whole number of stripe
/// pairs, so both tiles stand on the same ground.
const WRAPPED_SHIFT: f32 = 400.0;
const TILT_DEGREES: f32 = 12.0;
const CAMERA_DISTANCE: f32 = 12.0;
/// Room past the tile's box that a tilted projection may reach.
const OVERHANG: f32 = 24.0;
/// A channel difference a rasterizer's rounding cannot produce between two
/// pictures of one shape.
const CHANNEL_TOLERANCE: i32 = 6;
/// The page row the tiles never reach: the stripes are vertical, so it holds
/// the ground under every column.
const GROUND_ROW: f32 = 20.0;
/// How far past the wrapped tile's painted pixels an edge may differ in
/// coverage, in device pixels.
const EDGE_BAND: i64 = 2;
/// The share of the painted tile whose pixels may differ between the two.
const INTERIOR_MISMATCH_ALLOWED: f32 = 0.01;

static FAILED: AtomicBool = AtomicBool::new(false);

fn tilt() -> GraphicsLayer {
    GraphicsLayer {
        rotation_y: TILT_DEGREES,
        camera_distance: CAMERA_DISTANCE,
        ..Default::default()
    }
}

fn tile_glass() -> Glass {
    Glass::lens().shape(LiquidShape::RoundedRect(TILE_RADIUS))
}

fn tile_frame(left: f32) -> Modifier {
    Modifier::empty()
        .absolute_offset(left, TILE_TOP)
        .size(Size::new(TILE_WIDTH, TILE_HEIGHT))
}

fn rgb(shot: &RobotScreenshot, x: u32, y: u32) -> [i32; 3] {
    let at = ((y * shot.width + x) * 4) as usize;
    [0, 1, 2].map(|channel| i32::from(shot.pixels[at + channel]))
}

fn differs(a: [i32; 3], b: [i32; 3]) -> bool {
    (0..3).any(|channel| (a[channel] - b[channel]).abs() > CHANNEL_TOLERANCE)
}

/// How the merged tile compares with the wrapped one.
struct Comparison {
    /// Pixels the merged tile paints farther than [`EDGE_BAND`] from anything
    /// the wrapped tile paints: a surface drawn past the shape.
    leaked: usize,
    /// Pixels both tiles paint, differently.
    mismatched: usize,
    /// Pixels the wrapped tile paints.
    painted: usize,
}

fn compare(shot: &RobotScreenshot) -> Comparison {
    let scale = shot.width as f32 / shot.logical_width;
    let shift = (WRAPPED_SHIFT * scale) as u32;
    let left = ((MERGED_LEFT - OVERHANG) * scale) as u32;
    let right = ((MERGED_LEFT + TILE_WIDTH + OVERHANG) * scale) as u32;
    let top = ((TILE_TOP - OVERHANG) * scale) as u32;
    let bottom = ((TILE_TOP + TILE_HEIGHT + OVERHANG) * scale) as u32;
    let ground_row = (GROUND_ROW * scale) as u32;
    let (width, height) = ((right - left) as usize, (bottom - top) as usize);
    let mut merged_paint = vec![false; width * height];
    let mut wrapped_paint = vec![false; width * height];
    let mut mismatched = 0;
    for y in top..bottom {
        for x in left..right {
            let ground = rgb(shot, x, ground_row);
            let merged = rgb(shot, x, y);
            let wrapped = rgb(shot, x + shift, y);
            let at = (y - top) as usize * width + (x - left) as usize;
            merged_paint[at] = differs(merged, ground);
            wrapped_paint[at] = differs(wrapped, ground);
            mismatched += usize::from(
                merged_paint[at] && wrapped_paint[at] && differs(merged, wrapped),
            );
        }
    }
    let near_wrapped = |x: usize, y: usize| {
        (-EDGE_BAND..=EDGE_BAND).any(|dy| {
            (-EDGE_BAND..=EDGE_BAND).any(|dx| {
                let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                nx >= 0
                    && ny >= 0
                    && (nx as usize) < width
                    && (ny as usize) < height
                    && wrapped_paint[ny as usize * width + nx as usize]
            })
        })
    };
    let leaked = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .filter(|&(x, y)| merged_paint[y * width + x] && !near_wrapped(x, y))
        .count();
    Comparison {
        leaked,
        mismatched,
        painted: wrapped_paint.iter().filter(|&&painted| painted).count(),
    }
}

pub(crate) fn main() -> ExitCode {
    let shot_dir = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR")
            .unwrap_or_else(|_| "target/rotated-glass-shape".to_string()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create shot dir");

    AppLauncher::new()
        .with_title("Rotated glass shape")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_test_driver(move |robot| {
            robot_exit::arm_timeout(90);
            std::thread::sleep(Duration::from_millis(500));
            robot_shot::settle(&robot, 300);
            let shot = robot.screenshot().expect("tiles shot");
            robot_shot::save(&shot, &shot_dir, "tiles.png");
            let comparison = compare(&shot);
            println!(
                "[rotated-glass] leaked={} mismatched={} of painted={}",
                comparison.leaked, comparison.mismatched, comparison.painted
            );
            if comparison.leaked > 0 {
                robot_exit::fail_and_await_shutdown(
                    &robot,
                    &FAILED,
                    &format!(
                        "a glass node that carries its own tilt paints {} pixels past the \
                         shape the same glass under a tilted parent paints: the tilted \
                         surface leaks around its rounded shape",
                        comparison.leaked
                    ),
                );
            }
            let allowed = (comparison.painted as f32 * INTERIOR_MISMATCH_ALLOWED) as usize;
            if comparison.mismatched > allowed {
                robot_exit::fail_and_await_shutdown(
                    &robot,
                    &FAILED,
                    &format!(
                        "a glass node that carries its own tilt draws {} of its {} pixels \
                         unlike the same glass under a tilted parent, past the {allowed} \
                         edge coverage accounts for: the tilt or the glass differs",
                        comparison.mismatched, comparison.painted
                    ),
                );
            }
            println!("PASS: a tilted glass node draws its rounded shape and nothing around it");
            robot.exit().expect("exit");
        })
        .try_run(move || {
            LiquidStripedStage(WINDOW_WIDTH, WINDOW_HEIGHT, move || {
                CBox(
                    tile_frame(MERGED_LEFT)
                        .graphics_layer(tilt)
                        .glass_effect_with(tile_glass(), GlassDynamics::default),
                    BoxSpec::default(),
                    || {},
                );
                CBox(
                    tile_frame(MERGED_LEFT + WRAPPED_SHIFT).graphics_layer(tilt),
                    BoxSpec::default(),
                    || {
                        CBox(
                            Modifier::empty()
                                .fill_max_size()
                                .glass_effect_with(tile_glass(), GlassDynamics::default),
                            BoxSpec::default(),
                            || {},
                        );
                    },
                );
            });
        })
        .map_or(ExitCode::FAILURE, |()| robot_exit::exit_code(&FAILED))
}
