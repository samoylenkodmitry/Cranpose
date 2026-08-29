#[cfg(target_os = "ios")]
fn main() {
    desktop_app::ios_entry_point();
}

#[cfg(not(target_os = "ios"))]
fn main() {}
