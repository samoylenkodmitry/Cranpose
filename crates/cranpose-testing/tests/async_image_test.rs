//! `AsyncImage` through the headless robot: the placeholder shows while the
//! image decodes, the image then fills its box and decodes no larger than
//! the box, and bytes that are no image show the error.

use std::{
    io::Cursor,
    time::{Duration, Instant},
};

use cranpose::prelude::{AsyncImage, ImageRequest, image_loader};
use cranpose_testing::{RobotTestRule, TestRenderer, create_headless_robot_test};
use cranpose_ui::{ContentScale, Modifier, Size, Text, TextStyle};

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(width, height, image::Rgba([40, 120, 200, 255]))
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .expect("encode a png");
    bytes
}

fn screen(request: ImageRequest) -> RobotTestRule<TestRenderer> {
    create_headless_robot_test(200, 200, move || {
        AsyncImage(
            request.clone(),
            Some("Photo".into()),
            Modifier::empty().size(Size::new(40.0, 30.0)),
            ContentScale::Crop,
            || {
                Text("Loading", Modifier::empty(), TextStyle::default());
            },
            |_| {
                Text("Broken", Modifier::empty(), TextStyle::default());
            },
        );
    })
}

/// Frames until `done`, for five seconds of decoding at most.
fn settle_until(
    robot: &mut RobotTestRule<TestRenderer>,
    done: impl Fn(&mut RobotTestRule<TestRenderer>) -> bool,
) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(robot) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
        robot.wait_for_idle();
    }
    done(robot)
}

#[test]
fn the_placeholder_shows_then_the_image_at_the_size_of_its_box() {
    let bytes = png(400, 300);
    let mut robot = screen(ImageRequest::bytes("async-image-photo", bytes.clone()));
    assert!(robot.find_by_text("Loading").exists());
    assert!(settle_until(&mut robot, |robot| !robot
        .find_by_text("Loading")
        .exists()));
    let bitmap = image_loader()
        .cached(&ImageRequest::bytes("async-image-photo", bytes).max_size(40, 30))
        .expect("decoded at the box's size");
    assert_eq!((bitmap.width(), bitmap.height()), (40, 30));
    assert!(!robot.find_by_text("Broken").exists());
}

#[test]
fn bytes_that_are_no_image_show_the_error() {
    let mut robot = screen(ImageRequest::bytes(
        "async-image-broken",
        b"not an image".to_vec(),
    ));
    assert!(settle_until(&mut robot, |robot| robot
        .find_by_text("Broken")
        .exists()));
}
