use super::*;

#[test]
fn every_native_target_names_a_font_directory() {
    let directory = system_font_directory();
    if cfg!(target_arch = "wasm32") {
        assert!(directory.is_none(), "a page has no font directory to read");
    } else {
        let directory = directory.expect("a native target keeps its fonts somewhere");
        assert!(
            directory.is_absolute(),
            "a font directory is resolved from the root, not from wherever the app was \
             started: {}",
            directory.display()
        );
    }
}

#[test]
fn the_host_density_is_one_until_a_surface_has_been_measured() {
    assert!(
        host_density() > 0.0,
        "a density of zero would divide by zero"
    );
}
