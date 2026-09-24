use super::*;

fn loose() -> Constraints {
    Constraints {
        min_width: 0.0,
        max_width: 400.0,
        min_height: 0.0,
        max_height: 400.0,
    }
}

#[test]
fn a_small_control_grows_to_the_minimum_and_centres_its_content() {
    let result = minimum_interactive_placement(
        loose(),
        Size {
            width: 24.0,
            height: 20.0,
        },
    );
    assert_eq!((result.size.width, result.size.height), (48.0, 48.0));
    assert_eq!(
        (result.placement_offset_x, result.placement_offset_y),
        (12.0, 14.0)
    );
}

#[test]
fn a_large_control_keeps_its_own_size() {
    let result = minimum_interactive_placement(
        loose(),
        Size {
            width: 120.0,
            height: 56.0,
        },
    );
    assert_eq!((result.size.width, result.size.height), (120.0, 56.0));
    assert_eq!(
        (result.placement_offset_x, result.placement_offset_y),
        (0.0, 0.0)
    );
}

#[test]
fn a_tight_parent_wins_over_the_minimum() {
    let tight = Constraints {
        min_width: 0.0,
        max_width: 30.0,
        min_height: 0.0,
        max_height: 100.0,
    };
    let result = minimum_interactive_placement(
        tight,
        Size {
            width: 24.0,
            height: 24.0,
        },
    );
    assert_eq!((result.size.width, result.size.height), (30.0, 48.0));
    assert_eq!(
        (result.placement_offset_x, result.placement_offset_y),
        (3.0, 12.0)
    );
}
