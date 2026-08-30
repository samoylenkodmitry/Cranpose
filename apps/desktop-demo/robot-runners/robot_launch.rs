#![allow(dead_code)]

use cranpose::AppLauncher;

pub fn launch(title: &str, width: u32, height: u32) -> AppLauncher {
    AppLauncher::new()
        .with_title(title)
        .with_size(width, height)
        .with_headless(true)
}
