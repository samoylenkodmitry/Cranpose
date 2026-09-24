use super::*;

#[test]
fn a_picked_tab_says_so_and_takes_a_click() {
    let mut config = SemanticsConfiguration::default();

    selectable_semantics(true, Some(SemanticsWidgetRole::Tab))(&mut config);

    assert_eq!(config.selected, Some(true));
    assert_eq!(config.role, Some(SemanticsWidgetRole::Tab));
    assert!(config.is_clickable);
}

#[test]
fn a_choice_with_no_role_still_says_whether_it_is_picked() {
    let mut config = SemanticsConfiguration::default();

    selectable_semantics(false, None)(&mut config);

    assert_eq!(config.selected, Some(false));
    assert_eq!(config.role, None);
}
