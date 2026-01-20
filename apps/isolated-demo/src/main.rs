mod app;
mod fonts;

fn main() {
    #[cfg(feature = "logging")]
    let _ = env_logger::try_init();
    app::create_app().run(app::IsolatedDemoApp);
}
