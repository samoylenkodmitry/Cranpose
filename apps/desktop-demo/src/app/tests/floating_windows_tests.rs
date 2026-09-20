use super::*;

#[test]
fn a_toggle_offers_to_open_what_is_closed_and_to_close_what_is_open() {
    assert_eq!(
        toggle_label(false, "Open the pet", "Close the pet"),
        "Open the pet"
    );
    assert_eq!(
        toggle_label(true, "Open the pet", "Close the pet"),
        "Close the pet"
    );
}
