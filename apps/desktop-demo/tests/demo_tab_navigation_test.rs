use desktop_app::app::{DemoTab, DEMO_TABS, DESKTOP_INITIAL_TAB};

#[test]
fn desktop_demo_opens_on_the_liquid_playground() {
    assert_eq!(DESKTOP_INITIAL_TAB, DemoTab::Liquid);
}

#[test]
fn liquid_ui_is_a_front_row_demo_tab() {
    let liquid_index = DEMO_TABS
        .iter()
        .position(|tab| *tab == DemoTab::Liquid)
        .expect("Liquid UI must be registered in the desktop demo");

    assert!(
        liquid_index <= 1,
        "Liquid UI must be visible beside the default tab without scrolling; index={liquid_index}"
    );
}
