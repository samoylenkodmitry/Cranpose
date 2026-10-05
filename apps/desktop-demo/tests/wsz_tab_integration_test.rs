use desktop_app::app::{demo_tab_labels, startup_tab_from_args, DemoTab, DEMO_TABS};

#[test]
fn wsz_tab_is_registered_in_demo_tab_list() {
    assert!(
        DEMO_TABS.contains(&DemoTab::Wsz),
        "WSZ tab should be present in DEMO_TABS"
    );
}

#[test]
fn wsz_tab_can_be_selected_by_startup_name() {
    assert_eq!(DemoTab::Wsz.slug(), "wsz");
    assert_eq!(startup_tab_from_args(["wsz".to_owned()]), DemoTab::Wsz);
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(DemoTab::Wsz.source_path());
    assert!(source.is_file(), "WSZ source link should resolve");
}

#[test]
fn wsz_tab_label_is_stable() {
    assert_eq!(DemoTab::Wsz.label(), "WSZ");

    let labels = demo_tab_labels();
    assert!(
        labels.contains(&"WSZ"),
        "WSZ label should be discoverable from demo_tab_labels()"
    );
}
