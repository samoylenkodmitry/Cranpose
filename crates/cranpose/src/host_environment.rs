use std::path::Path;

/// Where this platform keeps the font files a system family is drawn from, or
/// `None` when there is no such directory to read.
///
/// The browser is the `None` case and not an oversight: a page has no
/// filesystem, and the fonts it can draw with are the ones the document already
/// names.
pub fn system_font_directory() -> Option<&'static Path> {
    let directory = if cfg!(target_os = "android") {
        cranpose_render_common::font_source::ANDROID_SYSTEM_FONT_DIR
    } else if cfg!(any(target_os = "macos", target_os = "ios")) {
        "/System/Library/Fonts"
    } else if cfg!(target_os = "windows") {
        "C:\\Windows\\Fonts"
    } else if cfg!(target_arch = "wasm32") {
        return None;
    } else {
        "/usr/share/fonts"
    };
    Some(Path::new(directory))
}

/// How many physical pixels this host puts in one logical pixel.
///
/// One number, from wherever this platform keeps it: the activity
/// configuration on Android, the screen's backing scale on macOS and iOS, the
/// window's scale factor on desktop, `devicePixelRatio` in a browser. An
/// application asking "how big is a hairline here" reads this rather than the
/// platform API behind it.
///
/// Before the first surface exists this is `1.0` — the honest answer, since
/// nothing has been measured yet — so a caller that needs the real value waits
/// for a frame or observes
/// [`rememberHostSurfaceSize`](cranpose_services::host_surface::rememberHostSurfaceSize).
pub fn host_density() -> f32 {
    cranpose_services::host_surface::host_surface_size().scale
}

#[cfg(test)]
mod tests {
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
}
