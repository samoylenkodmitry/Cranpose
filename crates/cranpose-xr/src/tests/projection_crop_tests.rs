use super::*;

const WHOLE: (u32, u32) = (1200, 1200);

// An eye at `x` meters on the room's x axis that looks straight ahead, 45 degrees each way.
fn eye(x: f32) -> RenderViewpoint {
    let quarter = std::f32::consts::FRAC_PI_4;
    RenderViewpoint {
        pose: Pose::at([x, 0.0, 0.0]),
        local_pose: Pose::at([x, 0.0, 0.0]),
        field_of_view: FieldOfView {
            angle_left: -quarter,
            angle_right: quarter,
            angle_up: quarter,
            angle_down: -quarter,
        },
    }
}

// A volume `distance` meters ahead whose content reaches `extent` dp from its center each way.
fn ahead(distance: f32, extent: [f32; 3]) -> RoomBox {
    RoomBox::of_volume(&Pose::at([0.0, 0.0, -distance]), extent)
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn a_box_ahead_covers_its_part_of_the_view() {
    // A page 400 by 200 mm, 20 mm deep, a meter ahead: its near face spans 0.2 / 0.99 each way
    let part = seen_part(&eye(0.0), &[ahead(1.0, [200.0, 100.0, 10.0])]).expect("a part");
    assert!(close(part.right, 0.2 / 0.99), "{part:?}");
    assert!(close(part.left, -0.2 / 0.99), "{part:?}");
    assert!(close(part.up, 0.1 / 0.99), "{part:?}");
    assert!(close(part.down, -0.1 / 0.99), "{part:?}");
}

#[test]
fn a_box_beside_the_eye_takes_the_whole_view() {
    // A box that reaches around the eye has corners behind it
    let around = RoomBox::cube([0.0, 0.0, -0.2], 0.5);
    assert_eq!(seen_part(&eye(0.0), &[around]), None);
    let mut size = LayerSize::default();
    let (images, _) = crop(
        &mut size,
        [&eye(-0.03), &eye(0.03)],
        Content::Boxes(&[around]),
        WHOLE,
    )
    .expect("a crop");
    assert_eq!(images, WHOLE);
}

#[test]
fn a_box_past_the_edge_of_the_view_counts_only_what_shows() {
    // A box far to the right reaches past the view's right edge, at a tangent of one
    let part = seen_part(&eye(0.0), &[RoomBox::cube([1.5, 0.0, -1.0], 0.2)]).expect("a part");
    assert!(close(part.right, 1.0), "{part:?}");
    assert!(
        part.left > 1.0 / 1.2 - 1e-3 && part.left < 1.3 / 0.8,
        "{part:?}"
    );
}

#[test]
fn a_small_page_takes_small_images_about_its_middle_in_each_eye() {
    let mut size = LayerSize::default();
    let page = [ahead(1.0, [200.0, 300.0, 20.0])];
    let (left, right) = (eye(-0.032), eye(0.032));
    let (images, regions) =
        crop(&mut size, [&left, &right], Content::Boxes(&page), WHOLE).expect("a crop");
    // The whole view spans 2 tangents in 1200 pixels: 600 pixels for a unit of tangent
    let focal = 600.0;
    assert!(close(regions[0].focal_length, focal));
    // The page spans about 0.4 by 0.6 tangents, so the images need about 245 by 365 pixels
    assert!(images.0 >= 245 && images.0 < 400, "{images:?}");
    assert!(images.1 >= 365 && images.1 <= 500, "{images:?}");
    assert_eq!(images.0 % 100, 0, "the width comes in steps: {images:?}");
    // Each eye's image stands about the page's middle as that eye sees it: the left eye sees it
    // to the right of its own axis
    for (region, eye_x) in regions.iter().zip([-0.032_f32, 0.032]) {
        let middle_x = region.left + images.0 as f32 / 2.0 / focal;
        let middle_y = region.top - images.1 as f32 / 2.0 / focal;
        assert!((middle_x + eye_x / 0.98).abs() < 1e-3, "{region:?}");
        assert!(middle_y.abs() < 1e-4, "{region:?}");
    }
}

#[test]
fn the_images_grow_at_once_and_shrink_only_after_the_need_stays_small() {
    let mut size = LayerSize::default();
    let first = size.next(Some((300, 300)), WHOLE);
    assert_eq!(first, (400, 400), "300 with room to spare, in steps of 100");
    assert_eq!(
        size.next(Some((500, 300)), WHOLE),
        (600, 400),
        "grows at once"
    );
    // A smaller need keeps the size for a while, so a page that moves keeps its swapchains
    for _ in 1..SHRINK_AFTER {
        assert_eq!(size.next(Some((100, 100)), WHOLE), (600, 400));
    }
    assert_eq!(size.next(Some((100, 100)), WHOLE), (200, 200));
    // A need that comes back before the wait ends starts the wait again
    size.next(Some((500, 300)), WHOLE);
    for _ in 1..SHRINK_AFTER {
        size.next(Some((100, 100)), WHOLE);
    }
    size.next(Some((500, 300)), WHOLE);
    assert_eq!(size.next(Some((100, 100)), WHOLE), (600, 400));
}

#[test]
fn a_need_near_the_whole_view_and_content_of_unknown_reach_take_the_whole_view() {
    let mut size = LayerSize::default();
    assert_eq!(size.next(Some((1100, 1000)), WHOLE), WHOLE);
    let mut size = LayerSize::default();
    assert_eq!(size.next(Some((200, 200)), WHOLE), (300, 300));
    assert_eq!(
        size.next(None, WHOLE),
        WHOLE,
        "the whole view comes at once"
    );
    let (images, regions) = crop(
        &mut LayerSize::default(),
        [&eye(-0.03), &eye(0.03)],
        Content::Whole,
        WHOLE,
    )
    .expect("a crop");
    assert_eq!(images, WHOLE);
    assert_eq!(
        regions[0],
        ImageRegion::whole(&eye(-0.03).field_of_view, WHOLE).expect("a region")
    );
}
