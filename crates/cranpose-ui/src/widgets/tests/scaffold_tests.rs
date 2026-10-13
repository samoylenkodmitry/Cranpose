use super::*;

#[test]
fn a_screen_can_keep_the_insets_it_draws_under() {
    let insets = PaddingValues::new(4.0, 24.0, 4.0, 48.0);
    let consumed = ScaffoldContentInsets {
        top: false,
        ..ScaffoldContentInsets::default()
    }
    .filter(insets);
    assert_eq!(consumed, PaddingValues::new(4.0, 0.0, 4.0, 48.0));
    assert_eq!(
        ScaffoldContentInsets::none().filter(insets),
        PaddingValues::default()
    );
}

#[test]
fn a_scaffold_states_no_surfaces_by_default() {
    assert_eq!(ScaffoldSpec::default().colors, ScaffoldColors::default());
    let colors = ScaffoldColors::uniform(Color(0.1, 0.1, 0.1, 1.0));
    assert_eq!(colors.background, colors.top_bar);
    assert_eq!(colors.background, colors.bottom_bar);
}
