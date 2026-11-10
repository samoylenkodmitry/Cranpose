fn main() {
    // The patchwork runs in a desktop window only.
    let desktop = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os != "android")
        && std::env::var("CARGO_CFG_TARGET_ARCH").is_ok_and(|arch| arch != "wasm32");
    let ui = if desktop {
        "ui/desktop.slint"
    } else {
        "ui/gauntlet.slint"
    };
    if let Err(error) = slint_build::compile(ui) {
        panic!("{ui}: {error}");
    }
}
