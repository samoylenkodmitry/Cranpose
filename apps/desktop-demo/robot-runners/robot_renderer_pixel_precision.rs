use cranpose::widgets::{Box, BoxSpec, Row, RowSpec};
use cranpose::{AppLauncher, Color, Modifier, Size};
use cranpose_testing::sample_screenshot_pixel_logical;
use cranpose_ui::round_scaling_list::CentreAnchor;
use cranpose_ui::widgets::wear::{
    rememberWearScalingListState, WearScalingLazyColumn, WearScalingLazyColumnSpec,
};

const WIDTH: u32 = 774;
const HEIGHT: u32 = 454;
const BG: Color = Color(4.0 / 255.0, 7.0 / 255.0, 11.0 / 255.0, 1.0);
const SOURCE: Color = Color(47.0 / 255.0, 168.0 / 255.0, 245.0 / 255.0, 0.82);
const OPAQUE: Color = Color(185.0 / 255.0, 242.0 / 255.0, 255.0 / 255.0, 1.0);

fn pixel(screenshot: &cranpose::RobotScreenshot, x: f32, y: f32) -> [u8; 3] {
    let rgba = sample_screenshot_pixel_logical(screenshot, x, y).expect("pixel in window");
    [rgba[0], rgba[1], rgba[2]]
}

fn main() {
    AppLauncher::new()
        .with_title("Renderer Pixel Precision")
        .with_size(WIDTH, HEIGHT)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(std::time::Duration::from_millis(900));
            let screenshot = robot.screenshot().expect("screenshot");
            let fractional = pixel(&screenshot, 80.0, 90.0);
            let faded = (0..screenshot.height)
                .flat_map(|y| (0..screenshot.width).map(move |x| (x, y)))
                .map(|(x, y)| pixel(&screenshot, x as f32, y as f32))
                .filter(|value| *value == [184, 241, 254])
                .count();
            assert_eq!(fractional, [39, 139, 203]);
            assert_eq!(faded, 0);
            robot.exit().expect("exit");
        })
        .run(content);
}

fn content() {
    Box(
        Modifier::empty().fill_max_size().background(BG),
        BoxSpec::default(),
        || {
            Row(
                Modifier::empty().fill_max_size(),
                RowSpec::default(),
                || {
                    Box(
                        Modifier::empty()
                            .fill_max_height()
                            .width(160.0)
                            .background(SOURCE),
                        BoxSpec::default(),
                        || {},
                    );
                    wear_list();
                },
            );
        },
    );
}

#[cranpose::composable]
fn wear_list() {
    let state = rememberWearScalingListState(CentreAnchor::default());
    WearScalingLazyColumn(
        Modifier::empty().size(Size::new(454.0, 454.0)),
        state,
        WearScalingLazyColumnSpec::default().content_padding(18.0, 34.0),
        |scope| {
            scope.items(9, |_| {
                Box(
                    Modifier::empty()
                        .fill_max_width()
                        .height(104.0)
                        .background(OPAQUE),
                    BoxSpec::default(),
                    || {},
                );
            });
        },
    );
}
