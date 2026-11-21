use super::*;
use crate::{
    StereoMode,
    space::{PanelKind, PanelSlot, Shape, Space},
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

// Registers a panel of `size` dp at `position`, facing +z. Returns its id.
fn put(space: &Space, shape: Shape, size: [f32; 2], position: [f32; 3]) -> u64 {
    let panel = space.new_panel();
    space.place(panel, size, Pose::IDENTITY, Pose::at(position));
    space.set_layer(panel.id, shape, 0);
    panel.id
}

// Registers a mono surface of `size` dp at `position`, facing +z. Returns its id.
fn put_surface(space: &Space, size: [f32; 2], position: [f32; 3]) -> u64 {
    let panel = space.new_panel();
    space.place_surface(
        panel,
        StereoMode::Mono,
        size,
        Pose::IDENTITY,
        Pose::at(position),
    );
    panel.id
}

// The ids of the panels in the order of their distance from the head, farthest first.
fn order(space: &Space, head: [f32; 3]) -> Vec<u64> {
    back_to_front(space, head)
        .iter()
        .map(|slot| slot.id)
        .collect()
}

// A space that shows curves, in which `scene` registers panels. The list order of the panels is
// not the order of their distances.
fn curved_scene(scene: impl FnOnce(&Space)) {
    let space = Space::new();
    space.set_curves_shown(true);
    cranpose_testing::run_test_composition(|_| scene(&space));
}

#[test]
fn curved_panels_draw_in_one_order_with_flat_panels_and_surfaces() {
    let curved = Shape::Curved { radius: 1.0 };
    // A curved panel 1 m ahead on a cylinder around the head, a flat panel in front of it and a
    // surface behind it
    curved_scene(|space| {
        let panel = put(space, curved, [2000.0, 1000.0], [0.0, 0.0, -1.0]);
        let near = put(space, Shape::Flat, [400.0, 300.0], [0.0, 0.0, -0.5]);
        let surface = put_surface(space, [1600.0, 900.0], [0.0, 0.0, -2.0]);
        assert_eq!(order(space, [0.0; 3]), [surface, panel, near]);
        // The head behind the surface sees them in the reverse order
        assert_eq!(order(space, [0.0, 0.0, -2.5]), [near, panel, surface]);
    });
    // A small curved panel at the corner of a large surface draws over it, as a flat one does,
    // although the corner lies farther from the head than the surface's center does
    curved_scene(|space| {
        let cursor = put(space, curved, [20.0, 20.0], [0.95, 0.5, -0.98]);
        let surface = put_surface(space, [2000.0, 1125.0], [0.0, 0.0, -1.0]);
        assert_eq!(order(space, [0.0; 3]), [surface, cursor]);
    });
    // A flat panel 3 cm in front of a curved panel of 0.4 m on a cylinder of 1 m stays in front
    // of it as the head steps aside
    curved_scene(|space| {
        let flat = put(space, Shape::Flat, [400.0, 300.0], [0.0, 0.0, -0.97]);
        let panel = put(space, curved, [400.0, 300.0], [0.0, 0.0, -1.0]);
        for x in [-0.2, 0.0, 0.2] {
            assert_eq!(order(space, [x, 0.0, 0.0]), [panel, flat], "head at {x} m");
        }
    });
}
