#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WebBackendPreference {
    Auto,
    WebGpu,
    Gl,
}

pub(super) fn requested_web_backend(window: &web_sys::Window) -> WebBackendPreference {
    let query = window.location().search().unwrap_or_default();
    for pair in query.trim_start_matches('?').split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if key != "backend" {
            continue;
        }
        return match value {
            "webgpu" => WebBackendPreference::WebGpu,
            "gl" => WebBackendPreference::Gl,
            _ => WebBackendPreference::Auto,
        };
    }
    WebBackendPreference::Auto
}

pub(super) fn instance_backends(preference: WebBackendPreference) -> wgpu::Backends {
    match preference {
        WebBackendPreference::Auto => wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL,
        WebBackendPreference::WebGpu => wgpu::Backends::BROWSER_WEBGPU,
        WebBackendPreference::Gl => wgpu::Backends::GL,
    }
}

// Cranpose's renderer needs no WebGL extension beyond the ones `glow` itself
// uses, and each one a page enables can wait for the browser's GPU process
// (forks/README.md). Code that renders with Cranpose's device, such as a game
// engine, says what it needs, and then gets every one.
pub(super) fn limit_webgl_extensions(requirements: crate::GpuRequirements) {
    if requirements == crate::GpuRequirements::default() {
        wgpu::hal::gles::ask_only_for_used_extensions();
    }
}

pub(super) fn required_limits_for_web_backend(
    backend: wgpu::Backend,
    adapter_limits: wgpu::Limits,
) -> wgpu::Limits {
    match backend {
        wgpu::Backend::BrowserWebGpu => wgpu::Limits::default().using_resolution(adapter_limits),
        _ => wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter_limits),
    }
}
