use super::*;
use crate::space::{Mobility, PanelKind, Ride, Shape};

// A panel of 800 by 500 dp a meter ahead. Its corner handle stands 0.44 meters to the right of
// its center and 0.29 meters below it.
fn panel() -> PanelSlot {
    PanelSlot {
        id: 1,
        kind: PanelKind::Content,
        atlas: [0.0, 0.0, 800.0, 500.0],
        pose: Pose::at([0.0, 0.0, -1.0]),
        mobility: Mobility::Fixed,
        shape: Shape::Flat,
        resize: Some(ResizeLimits::new()),
        room: [800.0, 500.0],
        priority: 0,
        ride: Ride::Free,
        tracked: None,
    }
}

// A ray from the origin through the point of the panel's plane `x` meters to the right and `y`
// meters above its center.
fn through(x: f32, y: f32) -> Ray {
    let length = (x * x + y * y + 1.0).sqrt();
    Ray {
        origin: [0.0; 3],
        direction: [x / length, y / length, -1.0 / length],
    }
}

// The size that a drag of the handle by `x` meters to the right and `y` meters up gives a panel
// that the ray grabbed at the middle of its handle.
fn dragged(limits: ResizeLimits, x: f32, y: f32) -> [f32; 2] {
    let slot = PanelSlot {
        resize: Some(limits),
        ..panel()
    };
    let resize = Resize::grab(&through(0.44, -0.29), &slot).expect("the panel has a handle");
    resize
        .size(&through(0.44 + x, -0.29 + y), &slot.pose)
        .expect("the ray meets the plane")
}

#[test]
fn a_drag_of_the_corner_moves_the_corner_with_the_center_in_place() {
    let free = ResizeLimits::new();
    assert_eq!(dragged(free, 0.0, 0.0), [800.0, 500.0]);
    // Each side moves by the drag, so the size changes by twice that
    assert_eq!(dragged(free, 0.1, -0.05), [1000.0, 600.0]);
    assert_eq!(dragged(free, -0.2, 0.1), [400.0, 300.0]);
}

#[test]
fn a_grab_beside_the_middle_of_the_handle_does_not_jump() {
    let slot = panel();
    let resize = Resize::grab(&through(0.46, -0.28), &slot).expect("the panel has a handle");
    assert_eq!(
        resize.size(&through(0.46, -0.28), &slot.pose),
        Some([800.0, 500.0])
    );
}

#[test]
fn the_size_stays_within_the_limits_on_each_axis() {
    let limits = ResizeLimits::new()
        .minimum_size(400.0, 250.0)
        .maximum_size(1000.0, 600.0);
    assert_eq!(dragged(limits, 0.3, 0.0), [1000.0, 500.0]);
    assert_eq!(dragged(limits, 0.3, -0.3), [1000.0, 600.0]);
    assert_eq!(dragged(limits, -0.9, 0.9), [400.0, 250.0]);
}

#[test]
fn a_minimum_wins_over_a_smaller_maximum() {
    let limits = ResizeLimits::new()
        .minimum_size(500.0, 300.0)
        .maximum_size(400.0, 200.0);
    for [x, y] in [[0.5, -0.5], [-0.9, 0.9], [0.0, 0.0]] {
        assert_eq!(dragged(limits, x, y), [500.0, 300.0]);
    }
}

#[test]
fn no_minimum_still_leaves_a_panel_that_the_user_can_grab() {
    assert_eq!(dragged(ResizeLimits::new(), -2.0, 2.0), [20.0, 20.0]);
}

#[test]
fn a_ray_that_grazes_the_plane_cannot_take_the_size_past_the_ceiling() {
    let free = ResizeLimits::new();
    assert_eq!(dragged(free, 1.0e4, 0.0), [4000.0, 500.0]);
    assert_eq!(dragged(free, 1.0e4, -1.0e4), [4000.0, 4000.0]);
    let kept = ResizeLimits::new().maintain_aspect_ratio(true);
    assert_eq!(dragged(kept, 1.0e4, 0.0), [4000.0, 2500.0]);
    // The ceiling is a maximum, so a minimum above it still wins
    let tall = ResizeLimits::new().minimum_size(5000.0, 0.0);
    assert_eq!(dragged(tall, 1.0, 0.0), [5000.0, 500.0]);
}

#[test]
fn kept_proportions_put_the_size_on_the_line_of_the_start_size() {
    let kept = ResizeLimits::new().maintain_aspect_ratio(true);
    // A drag to the right alone grows both sides by the share that is nearest to the drag
    assert_eq!(dragged(kept, 0.1, 0.0), [944.0, 590.0]);
    // A drag along the diagonal of the start size follows it exactly
    assert_eq!(dragged(kept, 0.1, -0.0625), [1000.0, 625.0]);
    assert_eq!(dragged(kept, -0.2, 0.125), [400.0, 250.0]);
}

#[test]
fn kept_proportions_stop_at_the_first_limit_that_they_meet() {
    let limits = ResizeLimits::new()
        .minimum_size(300.0, 100.0)
        .maximum_size(1200.0, 700.0)
        .maintain_aspect_ratio(true);
    // The height reaches its maximum first
    assert_eq!(dragged(limits, 0.8, -0.8), [1120.0, 700.0]);
    // The width reaches its minimum first
    assert_eq!(dragged(limits, -0.9, 0.9), [300.0, 188.0]);
}

#[test]
fn limits_that_the_proportions_cannot_meet_hold() {
    let limits = ResizeLimits::new()
        .minimum_size(0.0, 700.0)
        .maximum_size(1000.0, 1000.0)
        .maintain_aspect_ratio(true);
    assert_eq!(dragged(limits, 0.0, 0.0), [1000.0, 700.0]);
}

#[test]
fn a_ray_that_meets_the_plane_nowhere_gives_no_size() {
    let slot = panel();
    let resize = Resize::grab(&through(0.44, -0.29), &slot).expect("the panel has a handle");
    let ray = |direction| Ray {
        origin: [0.0; 3],
        direction,
    };
    assert_eq!(resize.size(&ray([1.0, 0.0, 0.0]), &slot.pose), None);
    assert_eq!(resize.size(&ray([0.0, 0.0, 1.0]), &slot.pose), None);
}

#[test]
fn a_panel_with_no_limits_has_no_handle_to_grab() {
    let fixed = PanelSlot {
        resize: None,
        ..panel()
    };
    assert_eq!(Resize::grab(&through(0.44, -0.29), &fixed), None);
    let curved = PanelSlot {
        shape: Shape::Curved { radius: 1.0 },
        ..panel()
    };
    assert_eq!(Resize::grab(&through(0.44, -0.29), &curved), None);
}
