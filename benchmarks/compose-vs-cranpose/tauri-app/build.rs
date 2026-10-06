fn main() {
    // The page is served from `desktop.py`'s address, so its calls to the
    // app's commands need permissions a capability grants.
    let manifest = tauri_build::AppManifest::new().commands(&["launch", "log"]);
    if let Err(error) =
        tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
    {
        eprintln!("tauri-build: {error:#}");
        std::process::exit(1);
    }
}
