pub(super) fn register() {
    cranpose_media::install();
    #[cfg(target_os = "macos")]
    crate::apple::media_source::register();
}
