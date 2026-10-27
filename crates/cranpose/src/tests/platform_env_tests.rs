use std::{cell::Cell, rc::Rc};

use cranpose_ui::{Configuration, EdgeInsets, UiModeType, local_safe_area_insets};

use super::PlatformEnvironment;

// The safe area content reads under a platform that reports `reported` on a
// device of `kind`.
fn content_safe_area(kind: UiModeType, reported: EdgeInsets) -> EdgeInsets {
    let env = PlatformEnvironment::new();
    env.set_configuration(Configuration { ui_mode_type: kind });
    env.set_safe_area(reported);
    let seen = Rc::new(Cell::new(EdgeInsets::default()));
    let sink = Rc::clone(&seen);
    let _composition = cranpose_ui::run_test_composition(move || {
        let sink = Rc::clone(&sink);
        env.compose_root(move || sink.set(local_safe_area_insets().current()));
    });
    seen.get()
}

#[test]
fn a_tv_keeps_content_inside_its_overscan_margins() {
    assert_eq!(
        content_safe_area(UiModeType::Television, EdgeInsets::default()),
        EdgeInsets::from_components(48.0, 27.0, 48.0, 27.0)
    );
    assert_eq!(
        content_safe_area(
            UiModeType::Television,
            EdgeInsets::from_components(80.0, 60.0, 80.0, 60.0)
        ),
        EdgeInsets::from_components(80.0, 60.0, 80.0, 60.0),
        "a wider reported margin stands"
    );
    assert_eq!(
        content_safe_area(UiModeType::Normal, EdgeInsets::default()),
        EdgeInsets::default(),
        "other devices keep what the platform reports"
    );
}
