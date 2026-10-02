use super::Control;

#[test]
fn reference_control_names_are_validated() {
    for name in [
        "slider",
        "toggle",
        "segmented",
        "button",
        "prominent-button",
        "chip",
        "filter-chip",
        "card",
        "menu",
        "icon-button",
        "search",
        "nav-bar",
        "list-row",
    ] {
        assert!(Control::parse(name).is_ok(), "{name}");
    }
    assert!(Control::parse("unknown").is_err());
}
