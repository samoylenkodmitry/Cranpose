use super::*;

#[test]
fn an_overlay_root_names_its_anchor_and_takes_the_host_size() {
    let root = HostOverlayRoot::new("editor");
    assert_eq!(root.anchor(), "editor");
    assert_eq!(root.layout_size(), Size::new(0.0, 0.0));

    root.set_size(Size::new(300.0, 120.0));
    assert_eq!(root.layout_size(), Size::new(300.0, 120.0));
    assert!(root.as_any().is::<HostOverlayRoot>());
}
