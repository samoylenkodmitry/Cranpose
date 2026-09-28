//! Captures every Compose-twin scene (see `test_screens::compose_twin`) to
//! `ROBOT_SHOT_DIR` and fails when a capture strays from its Compose Desktop
//! frame in `tools/compose-twin/reference` by more than antialiasing. The
//! modifier matrix's frames are compared cell by cell, against the known
//! differences in `tools/compose-twin/matrix-baseline.txt`.

use crate::output_paths;

use std::{collections::BTreeSet, path::PathBuf};

use cranpose::AppLauncher;
use desktop_app::test_screens::compose_twin::{
    ComposeTwinScreen, MATRIX_GRID, TWIN_CELL_STRAY_LIMIT, TWIN_FRAME_HEIGHT, TWIN_FRAME_WIDTH,
    TWIN_SCENE_STATE, TWIN_STRAY_LIMIT, TwinScene, TwinTolerance, twin_scenes, twin_stray_pixels,
};
use image::{RgbaImage, imageops};

const SHOT_DIR_ENV: &str = "ROBOT_SHOT_DIR";
const TWIN_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/compose-twin");

pub(crate) fn main() {
    let _ = env_logger::try_init();
    let shot_dir = std::env::var_os(SHOT_DIR_ENV).map_or_else(
        || output_paths::diagnostic_path("cranpose-compose-twin"),
        PathBuf::from,
    );
    std::fs::create_dir_all(&shot_dir)
        .unwrap_or_else(|err| panic!("failed to create {}: {err}", shot_dir.display()));

    AppLauncher::new()
        .with_title("Robot Compose Twin Scenes")
        .with_size(TWIN_FRAME_WIDTH, TWIN_FRAME_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(true)
        .with_robot_app_hook(set_scene_hook)
        .with_test_driver(move |robot| {
            let baseline = matrix_baseline();
            let mut failures = Vec::new();
            let mut differing = BTreeSet::new();
            for twin in twin_scenes() {
                let name = twin.name;
                robot
                    .invoke_app_hook("set-scene", name)
                    .unwrap_or_else(|err| panic!("failed to select scene '{name}': {err}"));
                robot
                    .pump_frames(3)
                    .unwrap_or_else(|err| panic!("failed to settle scene '{name}': {err}"));
                let shot = robot
                    .screenshot()
                    .unwrap_or_else(|err| panic!("screenshot failed for scene '{name}': {err}"));
                let image = RgbaImage::from_raw(shot.width, shot.height, shot.pixels)
                    .unwrap_or_else(|| panic!("scene '{name}' screenshot had an unexpected size"));
                let path = shot_dir.join(format!("{name}.png"));
                image
                    .save(&path)
                    .unwrap_or_else(|err| panic!("failed to write {}: {err}", path.display()));
                let reference_path = PathBuf::from(TWIN_DIR).join(format!("reference/{name}.png"));
                let reference = image::open(&reference_path)
                    .unwrap_or_else(|err| panic!("failed to read {}: {err}", reference_path.display()))
                    .to_rgba8();
                if twin.cells.is_empty() {
                    let stray = strays(&reference, &image, twin.tolerance, name);
                    println!("{name}: {stray} stray pixels (limit {TWIN_STRAY_LIMIT}) -> {}", path.display());
                    if stray > TWIN_STRAY_LIMIT {
                        failures.push(format!("{name}: {stray} stray pixels"));
                    }
                    continue;
                }
                differing.extend(differing_cells(twin, &reference, &image));
            }
            for cell in differing.difference(&baseline) {
                failures.push(format!("{cell} strays from Compose"));
            }
            for cell in baseline.difference(&differing) {
                failures.push(format!("{cell} matches now: drop it from matrix-baseline.txt"));
            }
            println!(
                "matrix: {} cells differ from Compose, {} of them known",
                differing.len(),
                differing.intersection(&baseline).count()
            );
            assert!(failures.is_empty(), "twin scenes stray from their Compose frames:\n{}", failures.join("\n"));
            println!("PASS: every twin scene matches its Compose frame");
            robot.exit().expect("exit");
        })
        .run(ComposeTwinScreen);
}

fn strays(reference: &RgbaImage, image: &RgbaImage, tolerance: TwinTolerance, what: &str) -> usize {
    twin_stray_pixels(reference.as_raw(), image.as_raw(), image.width() as usize, tolerance).unwrap_or_else(|| {
        panic!(
            "'{what}' is {}x{}, its Compose frame {}x{}",
            image.width(),
            image.height(),
            reference.width(),
            reference.height()
        )
    })
}

/// The cells of a matrix frame that stray past their limit, as
/// `frame / cell`, each printed with its count.
fn differing_cells(twin: &TwinScene, reference: &RgbaImage, image: &RgbaImage) -> Vec<String> {
    let limit = match twin.tolerance {
        TwinTolerance::Exact => 0,
        TwinTolerance::Edges => TWIN_CELL_STRAY_LIMIT,
    };
    twin.cells
        .iter()
        .enumerate()
        .filter_map(|(index, cell)| {
            let (x, y, width, height) = MATRIX_GRID.cell(index);
            let label = format!("{} / {cell}", twin.name);
            let expected = imageops::crop_imm(reference, x, y, width, height).to_image();
            let actual = imageops::crop_imm(image, x, y, width, height).to_image();
            let stray = strays(&expected, &actual, twin.tolerance, &label);
            (stray > limit).then(|| {
                println!("{label}: {stray} stray pixels (limit {limit})");
                label
            })
        })
        .collect()
}

/// The matrix cells known to differ from Compose, one `frame / cell` a line.
fn matrix_baseline() -> BTreeSet<String> {
    let path = PathBuf::from(TWIN_DIR).join("matrix-baseline.txt");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

fn set_scene_hook(name: String, argument: String) -> Result<Option<String>, String> {
    if name != "set-scene" {
        return Err(format!("unsupported robot app hook {name}({argument})"));
    }
    let index = twin_scenes()
        .position(|scene| scene.name == argument)
        .ok_or_else(|| format!("unknown twin scene '{argument}'"))?;
    let state = TWIN_SCENE_STATE
        .with(|cell| cell.borrow().as_ref().copied())
        .ok_or_else(|| "the twin screen was not composed before selecting a scene".to_string())?;
    state.set(index);
    Ok(None)
}
