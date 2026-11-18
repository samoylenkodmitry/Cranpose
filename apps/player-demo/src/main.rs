// The demo in a window of the desktop. `RUST_LOG=info` shows the decoder's counts each second.
fn main() {
    env_logger::init();
    cranpose::AppLauncher::new()
        .with_title("Player demo")
        .with_size(1280, 720)
        .run(player_demo::PlayerDemo);
}
