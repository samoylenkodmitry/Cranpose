use cranpose::widgets::{Box, BoxSpec, Column, ColumnSpec, Row, RowSpec};
use cranpose::{AppLauncher, Color, GraphicsLayer, Modifier};
use cranpose_testing::sample_screenshot_pixel_logical;

const WIDTH: u32 = 320;
const HEIGHT: u32 = 180;
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
            let faded = pixel(&screenshot, 240.0, 90.0);
            assert_eq!(fractional, [39, 139, 203]);
            assert_eq!(faded, [185, 242, 255]);
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
                    Box(
                        Modifier::empty()
                            .fill_max_height()
                            .width(160.0)
                            .graphics_layer_value(GraphicsLayer {
                                alpha: 254.9 / 255.0,
                                ..GraphicsLayer::default()
                            }),
                        BoxSpec::default(),
                        || {
                            Column(Modifier::empty(), ColumnSpec::default(), || {
                                Box(
                                    Modifier::empty().fill_max_size().background(OPAQUE),
                                    BoxSpec::default(),
                                    || {},
                                );
                            });
                        },
                    );
                },
            );
        },
    );
}
