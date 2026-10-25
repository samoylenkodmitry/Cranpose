//! On the web with the `gl-templates` feature, writes
//! `OUT_DIR/gl_templates.rs`: the GLSL templates of each built-in shader
//! module (`cranpose_gl_templates`), which a WebGL device builds pipelines
//! from instead of running naga on the page.

#![allow(
    clippy::disallowed_methods,
    reason = "a build script reads cargo's environment"
)]

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    #[cfg(feature = "gl-templates")]
    if std::env::var("CARGO_CFG_TARGET_ARCH").is_ok_and(|arch| arch == "wasm32")
        && let Err(error) = templates::write()
    {
        println!("cargo::error={error}");
    }
}

#[cfg(feature = "gl-templates")]
mod templates {
    use cranpose_ui_graphics::framework_shaders::MODULES;

    pub(super) fn write() -> Result<(), String> {
        let out = std::env::var_os("OUT_DIR").ok_or("OUT_DIR unset")?;
        let modules = MODULES
            .iter()
            .map(|&(name, wgsl)| {
                cranpose_gl_templates::module_templates(wgsl)
                    .map(|templates| (name, templates))
                    .map_err(|error| format!("{name}: {error}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        std::fs::write(
            std::path::Path::new(&out).join("gl_templates.rs"),
            cranpose_gl_templates::rust_source(&modules),
        )
        .map_err(|error| error.to_string())
    }
}
