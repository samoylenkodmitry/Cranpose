use std::path::Path;

const FALLBACK_APPLICATION_ID: &str = "cranpose-app";

pub(crate) fn register(configured: Option<&str>) {
    let executable = std::env::current_exe().ok();
    let application_id = resolve(configured, executable.as_deref());
    if let Err(error) = cranpose_services::set_application_id(&application_id) {
        log::warn!("cranpose: `{application_id}` is not a usable application id: {error}");
        return;
    }
    crate::pipeline_cache_file::publish();
}

pub(crate) fn resolve(configured: Option<&str>, executable: Option<&Path>) -> String {
    configured
        .map(str::to_owned)
        .or_else(|| {
            executable
                .and_then(Path::file_stem)
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| FALLBACK_APPLICATION_ID.to_owned())
}

#[cfg(test)]
#[path = "tests/application_id_tests.rs"]
mod tests;
