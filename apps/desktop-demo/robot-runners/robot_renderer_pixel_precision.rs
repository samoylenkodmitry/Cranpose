use cranpose::widgets::{Box, BoxSpec, Row, RowSpec};
use cranpose::LazyItems;
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
const OPAQUE: Color = Color(185.0 / 255.0, 242.0 / 255.0, 1.0, 1.0);

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
            let mut opaque = 0usize;
            let mut opaque_bounds = [screenshot.width, screenshot.height, 0, 0];
            for y in 0..screenshot.height {
                for x in 0..screenshot.width {
                    if pixel(&screenshot, x as f32, y as f32) == [185, 242, 255] {
                        opaque += 1;
                        opaque_bounds[0] = opaque_bounds[0].min(x);
                        opaque_bounds[1] = opaque_bounds[1].min(y);
                        opaque_bounds[2] = opaque_bounds[2].max(x);
                        opaque_bounds[3] = opaque_bounds[3].max(y);
                    }
                }
            }
            assert_eq!(fractional, [39, 139, 203]);
            assert_eq!(faded, 0);
            assert!(opaque > 100_000);
            assert_eq!(opaque_bounds, [179, 68, 595, 386]);
            assert_eq!(pixel(&screenshot, 300.0, 90.0), [185, 242, 255]);
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
            scope.items(LazyItems::new(9).key(|index: usize| index as u64), |_| {
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
