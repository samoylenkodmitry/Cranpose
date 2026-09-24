#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    #[cfg(feature = "logging")]
    let _ = env_logger::try_init();
    if let Err(error) =
        coroflow_demo::ui::app::create_app().try_run(coroflow_demo::ui::app::CoroflowDemoApp)
    {
        eprintln!("Coroflow Notes failed to start: {error}");
        std::process::exit(1);
    }
}
