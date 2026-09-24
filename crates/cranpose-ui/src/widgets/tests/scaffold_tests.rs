use super::*;

#[test]
fn padding_values_preserve_each_edge() {
    assert_eq!(
        PaddingValues::new(1.0, 2.0, 3.0, 4.0),
        PaddingValues {
            start: 1.0,
            top: 2.0,
            end: 3.0,
            bottom: 4.0,
        }
    );
    assert_eq!(
        PaddingValues::all(6.0),
        PaddingValues::new(6.0, 6.0, 6.0, 6.0)
    );
}

#[test]
fn reading_order_padding_swaps_sides_in_a_right_to_left_layout() {
    let padding = PaddingValues::new(24.0, 2.0, 8.0, 4.0);
    assert_eq!(
        padding.physical(LayoutDirection::Ltr),
        EdgeInsets::from_components(24.0, 2.0, 8.0, 4.0)
    );
    assert_eq!(
        padding.physical(LayoutDirection::Rtl),
        EdgeInsets::from_components(8.0, 2.0, 24.0, 4.0)
    );
}

#[test]
fn merging_takes_the_larger_of_each_edge() {
    let bars = PaddingValues::new(0.0, 56.0, 0.0, 0.0);
    let insets = PaddingValues::new(0.0, 24.0, 0.0, 48.0);
    assert_eq!(insets.max(bars), PaddingValues::new(0.0, 56.0, 0.0, 48.0));
}

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
