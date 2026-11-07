use cranpose_gl_templates::first_screen_glsl;
use cranpose_ui_graphics::framework_shaders::{MODULES, SOURCES_KEY};

const BLIT: &str = "BLIT|blit_vs|blit_fs|BLIT_UNMASKED_NEAREST=1";

#[test]
fn a_stage_of_a_module_the_framework_lacks_fails_the_file() {
    let error = first_screen_glsl(&MODULES, SOURCES_KEY, "RETIRED|vs|fs|").unwrap_err();
    assert!(error.contains("RETIRED"), "{error}");
}

#[test]
fn a_constant_the_shader_lacks_fails_the_file() {
    let listed = "BLIT|blit_vs|blit_fs|RETIRED_CONSTANT=1";
    let error = first_screen_glsl(&MODULES, SOURCES_KEY, listed).unwrap_err();
    assert!(error.contains("RETIRED_CONSTANT"), "{error}");
}

#[test]
fn a_program_two_runs_listed_is_written_once() {
    let once = first_screen_glsl(&MODULES, SOURCES_KEY, BLIT).unwrap();
    let twice = first_screen_glsl(&MODULES, SOURCES_KEY, &format!("{BLIT}\n\n{BLIT}\n")).unwrap();
    assert_eq!(once, twice);
}

#[test]
fn the_file_names_the_shaders_it_was_written_from() {
    let file = first_screen_glsl(&MODULES, SOURCES_KEY, BLIT).unwrap();
    let header = format!("cranpose-first-screen-glsl/1/{SOURCES_KEY}");
    assert!(
        file.starts_with(&format!("{}:{header}", header.len())),
        "{file}"
    );
}
