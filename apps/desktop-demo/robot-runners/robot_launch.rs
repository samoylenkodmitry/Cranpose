use cranpose::AppLauncher;

pub fn launch(title: &str, width: u32, height: u32) -> AppLauncher {
    AppLauncher::new()
        .with_title(title)
        .with_size(width, height)
        .with_headless(true)
}

pub fn counter_demo() {
    desktop_app::app::combined_app_with_initial_tab(Some(desktop_app::app::DemoTab::Counter));
}
