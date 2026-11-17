use super::*;
use crate::{
    StereoMode,
    space::{PanelKind, PanelSlot, Space},
};

// The panels in the order of their distance from the head, farthest first.
fn back_to_front(space: &Space, head: [f32; 3]) -> Vec<PanelSlot> {
    let mut slots = space.panels();
    sort_back_to_front(&mut slots, head, |slot| &slot.pose);
    slots
}

// A desktop surface, 2 m wide, a meter ahead of the head, and a cursor panel 2 cm in front of it.
// Registering the cursor first makes the list order the wrong one.
fn desktop_and_cursor(cursor: Pose) -> Space {
    let space = Space::new();
    cranpose_testing::run_test_composition(|_| {
        let (cursor_panel, surface) = (space.new_panel(), space.new_panel());
        space.place(cursor_panel, [20.0, 20.0], Pose::IDENTITY, cursor);
        space.place_surface(
            surface,
            StereoMode::Mono,
            [2000.0, 1125.0],
            Pose::IDENTITY,
            Pose::at([0.0, 0.0, -1.0]),
        );
    });
    space
}

#[test]
fn a_panel_in_front_of_a_surface_draws_after_it() {
    // At the corner of the surface the cursor's center is farther from the head than the
    // surface's center is.
    let corner = Pose::at([0.95, 0.5, -0.98]);
    let space = desktop_and_cursor(corner);
    let head = [0.0, 0.0, 0.0];
    let order = back_to_front(&space, head);
    assert!(matches!(order[0].kind, PanelKind::Surface(_)));
    assert_eq!(order[1].pose, corner);

    // The head on the other side of the surface sees the cursor behind it
    let behind = back_to_front(&space, [0.0, 0.0, -2.0]);
    assert_eq!(behind[0].pose, corner);
    assert!(matches!(behind[1].kind, PanelKind::Surface(_)));
}
