use std::sync::Arc;

use cranpose_services::{HostController, HostControllerRef, PlatformDirectories};

struct Languages;

impl HostController for Languages {
    fn set_keep_screen_on(&self, _: bool) {}
    fn platform_directories(&self) -> Option<PlatformDirectories> {
        None
    }
    fn exit(&self) {}
    fn background(&self) {}
    fn preferred_languages(&self) -> Vec<String> {
        ["invalid!", "fr-CA", "ar-EG-u-nu-arab"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    }
}

struct RestoreHost(Option<HostControllerRef>);

impl Drop for RestoreHost {
    fn drop(&mut self) {
        match self.0.take() {
            Some(host) => cranpose_services::set_host_controller(host),
            None => cranpose_services::clear_host_controller(),
        }
    }
}

#[test]
fn native_host_preferences_override_os_queries_and_supply_composition_defaults() {
    let _restore = RestoreHost(cranpose_services::host_controller());
    cranpose_services::set_host_controller(Arc::new(Languages));
    let expected: Vec<_> = ["fr-CA", "ar-EG"]
        .into_iter()
        .map(|tag| cranpose::localization::Locale::parse(tag).expect("locale"))
        .collect();
    assert_eq!(cranpose::system_languages(), expected);
    cranpose_ui::run_test_composition(|| {
        assert_eq!(
            cranpose::local_system_languages().current().as_ref(),
            expected
        );
    });
}
