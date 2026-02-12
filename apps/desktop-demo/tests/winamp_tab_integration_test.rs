use desktop_app::app::{demo_tab_labels, DemoTab, DEMO_TABS};

#[test]
fn winamp_tab_is_registered_in_demo_tab_list() {
    assert!(
        DEMO_TABS.contains(&DemoTab::Winamp),
        "Winamp tab should be present in DEMO_TABS"
    );
}

#[test]
fn winamp_tab_label_is_stable() {
    assert_eq!(DemoTab::Winamp.label(), "Winamp");

    let labels = demo_tab_labels();
    assert!(
        labels.contains(&"Winamp"),
        "Winamp label should be discoverable from demo_tab_labels()"
    );
}
