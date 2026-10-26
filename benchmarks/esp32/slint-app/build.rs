fn main() {
    embuild::espidf::sysenv::output();
    let config = slint_build::CompilerConfiguration::new()
        .embed_resources(slint_build::EmbedResourcesKind::EmbedForSoftwareRenderer);
    if let Err(error) = slint_build::compile_with_config("ui/dashboard.slint", config) {
        panic!("ui/dashboard.slint: {error}");
    }
}
