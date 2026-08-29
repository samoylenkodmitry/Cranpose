//! Round and heavily-rounded liquid-glass surfaces must refract their
//! backdrop smoothly out to the edge, not pinch it toward a single point.
//!
//! wcKSRD's transmission displacement pulls each pixel's source sample
//! toward the shape's optical axis as it nears the rim (`lens_scale -> 0`).
//! The pull's reach used to scale with the raw distance from that axis
//! instead of the lens's own optical depth (`lens_refraction`), so on a
//! full circle or a capsule's rounded cap -- where every rim point is far
//! from the axis -- a rim pixel could sample all the way back to the
//! shape's own center. Content near the center leaks out to the rim,
//! reading as a pinwheel/moire swirl on a checkerboard and, more simply,
//! as a small marker placed dead center becoming visible at the rim.
//!
//! This renders a circle and a capsule, each centered over a plain
//! backdrop with one small, uniquely colored marker at the shape's own
//! center, and asserts the marker never reads at any point around the
//! rim: with the reach bounded to lens_refraction, the closest any rim
//! pixel can pull toward center is `radius - lens_refraction`, which for
//! these shapes' sizes stays far outside the marker.
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_liquid_round_glass_edge_refraction --features desktop,robot-app
//! ```

use std::{
    f32::consts::TAU,
    path::PathBuf,
    process::ExitCode,
    sync::atomic::{AtomicBool, Ordering},
};

use cranpose::{
    liquid::prelude::*,
    widgets::{Box as CBox, BoxSpec},
    AppLauncher, Brush, Color, Modifier, Point, Size,
};

static FAILED: AtomicBool = AtomicBool::new(false);

const CANVAS_WIDTH: f32 = 820.0;
const CANVAS_HEIGHT: f32 = 300.0;
const ROW_Y: f32 = CANVAS_HEIGHT * 0.5;

const CIRCLE_DIAMETER: f32 = 260.0;
const CIRCLE_RADIUS: f32 = CIRCLE_DIAMETER * 0.5;
const CIRCLE_CENTER_X: f32 = 150.0;

// Same height (so the same inradius, and thus the same lens_refraction
// reach) as the circle's diameter — a capsule is "a circle with a body
// stretched between its two rounded caps", and giving both the same
// curvature keeps the two probes' rim geometry directly comparable.
const CAPSULE_HEIGHT: f32 = CIRCLE_DIAMETER;
const CAPSULE_WIDTH: f32 = 460.0;
const CAPSULE_CENTER_X: f32 = 550.0;
/// The capsule's rounded caps sit this far either side of its own center —
/// its half-width less its half-height, the flat body it wraps around.
const CAPSULE_CAP_OFFSET: f32 = CAPSULE_WIDTH * 0.5 - CAPSULE_HEIGHT * 0.5;

const MARKER_RADIUS: f32 = 10.0;
const RIM_SAMPLE_COUNT: u32 = 36;
/// A rim sample this close to the shape's true boundary line still lands
/// inside the defect's compressed band (see `probe_glass`'s refraction
/// depth) while reading through the coverage ramp's already-opaque
/// interior, not its faded antialiased edge.
const RIM_INSET: f32 = 2.0;

const BACKDROP: Color = Color(0.5, 0.5, 0.5, 1.0);
const MARKER: Color = Color(1.0, 0.0, 1.0, 1.0);

struct Probe {
    name: &'static str,
    center: (f32, f32),
    /// The rim-sampling radius: the shape's own radius for a circle, or a
    /// capsule's half-height (its rounded caps' true radius).
    radius: f32,
    /// Extra x-offset applied per sample direction's sign, tracing a
    /// capsule's flat sides and both rounded caps in one loop; zero for a
    /// circle.
    cap_offset: f32,
}

fn probes() -> [Probe; 2] {
    [
        Probe {
            name: "circle",
            center: (CIRCLE_CENTER_X, ROW_Y),
            radius: CIRCLE_RADIUS,
            cap_offset: 0.0,
        },
        Probe {
            name: "capsule",
            center: (CAPSULE_CENTER_X, ROW_Y),
            radius: CAPSULE_HEIGHT * 0.5,
            cap_offset: CAPSULE_CAP_OFFSET,
        },
    ]
}

fn probe_glass(shape: LiquidShape) -> Glass {
    Glass::lens()
        .shape(shape)
        .blur_radius(0.0)
        .dispersion(0.0)
        .highlight(0.0)
        .tint(Color::TRANSPARENT)
        // Widens the compressed-toward-center band the defect lives in
        // (lens_refraction = inradius * this) from a sub-pixel sliver at
        // the reference default to a few px, so RIM_INSET lands inside it
        // reliably instead of needing sub-pixel precision. Still well
        // inside Glass::refraction_depth's documented 0..2 range.
        .refraction_depth(0.5)
}

#[cranpose::composable]
#[allow(non_snake_case)]
fn ProbeApp() {
    LiquidTheme(LiquidThemeSpec::default(), || {
        CBox(
            Modifier::empty()
                .size(Size::new(CANVAS_WIDTH, CANVAS_HEIGHT))
                .draw_behind(|scope| {
                    scope.draw_rect(Brush::solid(BACKDROP));
                    for probe in probes() {
                        scope.draw_circle(
                            Brush::solid(MARKER),
                            Point { x: probe.center.0, y: probe.center.1 },
                            MARKER_RADIUS,
                        );
                    }
                }),
            BoxSpec::default(),
            || {
                CBox(
                    Modifier::empty()
                        .offset(CIRCLE_CENTER_X - CIRCLE_RADIUS, ROW_Y - CIRCLE_RADIUS)
                        .size(Size::new(CIRCLE_DIAMETER, CIRCLE_DIAMETER))
                        .glass_effect(probe_glass(LiquidShape::Circle)),
                    BoxSpec::default(),
                    || {},
                );
                CBox(
                    Modifier::empty()
                        .offset(
                            CAPSULE_CENTER_X - CAPSULE_WIDTH * 0.5,
                            ROW_Y - CAPSULE_HEIGHT * 0.5,
                        )
                        .size(Size::new(CAPSULE_WIDTH, CAPSULE_HEIGHT))
                        .glass_effect(probe_glass(LiquidShape::Capsule)),
                    BoxSpec::default(),
                    || {},
                );
            },
        );
    });
}

fn color_distance(a: (u8, u8, u8), b: (u8, u8, u8)) -> f32 {
    let dr = a.0 as f32 - b.0 as f32;
    let dg = a.1 as f32 - b.1 as f32;
    let db = a.2 as f32 - b.2 as f32;
    (dr * dr + dg * dg + db * db).sqrt()
}

fn main() -> ExitCode {
    let _ = env_logger::try_init();
    let shot_dir = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR")
            .unwrap_or_else(|_| "target/liquid-round-glass-edge-refraction".to_string()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create shot dir");

    AppLauncher::new()
        .with_title("Round Glass Edge Refraction")
        .with_size(CANVAS_WIDTH as u32, CANVAS_HEIGHT as u32)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_test_driver(move |robot| {
            std::thread::sleep(std::time::Duration::from_millis(700));
            let _ = robot.wait_for_idle();
            let shot = robot.screenshot().expect("shot");
            if let Some(image) =
                image::RgbaImage::from_raw(shot.width, shot.height, shot.pixels.clone())
            {
                let _ = image.save(shot_dir.join("round-glass-edge-refraction.png"));
            }
            let scale = shot.width as f32 / shot.logical_width;
            let sample = |lx: f32, ly: f32| -> (u8, u8, u8) {
                let px = (lx * scale).round().clamp(0.0, shot.width as f32 - 1.0) as u32;
                let py = (ly * scale).round().clamp(0.0, shot.height as f32 - 1.0) as u32;
                let idx = ((py * shot.width + px) * 4) as usize;
                (shot.pixels[idx], shot.pixels[idx + 1], shot.pixels[idx + 2])
            };
            let marker_rgb = (
                (MARKER.r() * 255.0) as u8,
                (MARKER.g() * 255.0) as u8,
                (MARKER.b() * 255.0) as u8,
            );

            for probe in probes() {
                // Sanity: the marker itself must actually be rendered (a
                // deep, undistorted lens face is identity-mapped), or the
                // rim check below would trivially pass by testing nothing.
                let at_marker = sample(probe.center.0, probe.center.1);
                let marker_visible = color_distance(at_marker, marker_rgb);
                if marker_visible > 60.0 {
                    println!(
                        "\n✗ the {}'s own center marker is not visible through the glass \
                         ({at_marker:?} against marker {marker_rgb:?}, distance \
                         {marker_visible:.1}) -- the probe is not testing anything",
                        probe.name
                    );
                    FAILED.store(true, Ordering::Relaxed);
                }

                // The core assertion: no point around the shape's rim may
                // read as its own center marker's color.
                let mut worst = f32::MAX;
                let mut worst_angle = 0.0f32;
                for step in 0..RIM_SAMPLE_COUNT {
                    let angle = (step as f32) * TAU / RIM_SAMPLE_COUNT as f32;
                    let axis_shift = probe.cap_offset * angle.cos().signum();
                    let lx = probe.center.0 + axis_shift + (probe.radius - RIM_INSET) * angle.cos();
                    let ly = probe.center.1 + (probe.radius - RIM_INSET) * angle.sin();
                    let rgb = sample(lx, ly);
                    let dist = color_distance(rgb, marker_rgb);
                    if dist < worst {
                        worst = dist;
                        worst_angle = angle.to_degrees();
                    }
                }
                const NOT_MARKER_FLOOR: f32 = 80.0;
                println!(
                    "{}: closest rim sample to the marker color = {worst:.1} (at {worst_angle:.0}deg)",
                    probe.name
                );
                if worst < NOT_MARKER_FLOOR {
                    println!(
                        "\n✗ the {}'s rim reads within {worst:.1} of its own center marker's \
                         color (floor {NOT_MARKER_FLOOR}) at {worst_angle:.0} degrees around the \
                         perimeter -- the center is bleeding out to the edge instead of the \
                         glass refracting smoothly out to it",
                        probe.name
                    );
                    FAILED.store(true, Ordering::Relaxed);
                }
            }

            if !FAILED.load(Ordering::Relaxed) {
                println!(
                    "PASS: neither the circle's nor the capsule's rim shows its own center \
                     bleeding through"
                );
            }
            robot.exit().expect("exit");
        })
        .try_run(move || ProbeApp())
        .expect("launch round glass edge refraction runner");

    if FAILED.load(Ordering::Relaxed) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
