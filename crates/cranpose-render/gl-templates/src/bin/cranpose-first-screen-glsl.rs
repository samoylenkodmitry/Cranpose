//! `cranpose-first-screen-glsl STAGES OUT` writes OUT, the exact GLSL of the
//! WebGL stages listed in STAGES by a recording run of a page's first screen
//! (`scripts/web/record_first_screen.mjs`). A page built with first-screen
//! records (`CRANPOSE_FIRST_SCREEN_PIPELINES`) fetches OUT from beside its
//! wasm as `first_screen_glsl.txt`.

#![allow(clippy::print_stderr, reason = "a command-line tool prints its errors")]

use std::process::ExitCode;

use cranpose_ui_graphics::framework_shaders::{MODULES, SOURCES_KEY};

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [stages, out] = &arguments[..] else {
        eprintln!("usage: cranpose-first-screen-glsl STAGES OUT");
        return ExitCode::from(2);
    };
    let written = std::fs::read_to_string(stages)
        .map_err(|error| format!("{stages}: {error}"))
        .and_then(|listed| cranpose_gl_templates::first_screen_glsl(&MODULES, SOURCES_KEY, &listed))
        .and_then(|file| std::fs::write(out, file).map_err(|error| format!("{out}: {error}")));
    match written {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cranpose-first-screen-glsl: {error}");
            ExitCode::FAILURE
        }
    }
}
