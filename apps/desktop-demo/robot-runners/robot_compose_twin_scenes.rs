//! Captures every Compose-twin scene (see `test_screens::compose_twin`) to
//! `ROBOT_SHOT_DIR` and fails when a capture strays from its Compose Desktop
//! frame in `tools/compose-twin/reference` by more than antialiasing.

use crate::output_paths;

use std::path::PathBuf;

use cranpose::AppLauncher;
use desktop_app::test_screens::compose_twin::{
    ComposeTwinScreen, TWIN_FRAME_HEIGHT, TWIN_FRAME_WIDTH, TWIN_SCENES, TWIN_SCENE_STATE,
    TWIN_STRAY_LIMIT, twin_stray_pixels,
};
use image::RgbaImage;

const SHOT_DIR_ENV: &str = "ROBOT_SHOT_DIR";
const REFERENCE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/compose-twin/reference");

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
            let mut failures = 0;
            for (name, _) in TWIN_SCENES {
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
                let reference_path = PathBuf::from(REFERENCE_DIR).join(format!("{name}.png"));
                let reference = image::open(&reference_path)
                    .unwrap_or_else(|err| panic!("failed to read {}: {err}", reference_path.display()))
                    .to_rgba8();
                let stray = twin_stray_pixels(reference.as_raw(), image.as_raw(), image.width() as usize)
                    .unwrap_or_else(|| {
                        panic!(
                            "scene '{name}' is {}x{}, its Compose frame {}x{}",
                            image.width(),
                            image.height(),
                            reference.width(),
                            reference.height()
                        )
                    });
                println!("{name}: {stray} stray pixels (limit {TWIN_STRAY_LIMIT}) -> {}", path.display());
                failures += usize::from(stray > TWIN_STRAY_LIMIT);
            }
            assert_eq!(failures, 0, "{failures} twin scenes stray from their Compose frames");
            println!("PASS: {} twin scenes match their Compose frames", TWIN_SCENES.len());
            robot.exit().expect("exit");
        })
        .run(ComposeTwinScreen);
}

fn set_scene_hook(name: String, argument: String) -> Result<Option<String>, String> {
    if name != "set-scene" {
        return Err(format!("unsupported robot app hook {name}({argument})"));
    }
    let index = TWIN_SCENES
        .iter()
        .position(|(scene, _)| *scene == argument)
        .ok_or_else(|| format!("unknown twin scene '{argument}'"))?;
    let state = TWIN_SCENE_STATE
        .with(|cell| cell.borrow().as_ref().copied())
        .ok_or_else(|| "the twin screen was not composed before selecting a scene".to_string())?;
    state.set(index);
    Ok(None)
}
