#[cfg(target_os = "ios")]
fn main() {
    if let Err(error) =
        coroflow_demo::ui::app::create_app().try_run(coroflow_demo::ui::app::CoroflowDemoApp)
    {
        eprintln!("Coroflow Notes failed to start: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "ios"))]
fn main() {}
