mod output_paths;

use std::cell::Cell;

use cranpose::{AppLauncher, Brush, Color, Modifier, Size};
use cranpose_core::{rememberMutableStateOf, MutableState};
use cranpose_ui::{
    composable,
    widgets::{Box, BoxSpec, Column, ColumnSpec, Row, RowSpec},
};
use cranpose_ui_graphics::{CompositingStrategy, GraphicsLayer};

const TITLE: &str = "Robot Viewport Uniform Growth";
const COLUMNS: u32 = 16;
const ROWS: u32 = 20;
const TILE: u32 = 16;
const INK: [u8; 3] = [32, 160, 224];

thread_local! {
    static ROW_COUNT: Cell<Option<MutableState<u32>>> = const { Cell::new(None) };
}

fn main() {
    let _ = env_logger::try_init();
    AppLauncher::new()
        .with_title(TITLE)
        .with_size(COLUMNS * TILE, ROWS * TILE)
        .with_headless(true)
        .with_robot_app_hook(|name, _| {
            if name != "expand" {
                return Err(format!("unknown hook {name}"));
            }
            ROW_COUNT.with(|cell| cell.get().expect("row state").set(ROWS));
            Ok(None)
        })
        .with_test_driver(|robot| {
            robot.wait_for_idle().expect("initial frame settles");
            robot.invoke_app_hook("expand", "").expect("expand grid");
            robot.wait_for_idle().expect("expanded frame settles");
            let path = output_paths::diagnostic_path("viewport-uniform-growth.png");
            let capture = robot.screenshot().expect("first expanded frame");
            let frame = image::RgbaImage::from_raw(capture.width, capture.height, capture.pixels).expect("RGBA capture");
            frame.save(&path).expect("save capture");
            for index in 0..COLUMNS * ROWS {
                let x = (index % COLUMNS * 2 + 1) * frame.width() / (COLUMNS * 2);
                let y = (index / COLUMNS * 2 + 1) * frame.height() / (ROWS * 2);
                assert_eq!(
                    &frame.get_pixel(x, y).0[..3],
                    &[INK[0] + (index % COLUMNS) as u8, INK[1] + (index / COLUMNS) as u8, INK[2]],
                    "tile {index} is missing from the first expanded frame after uniform storage grows; capture={}",
                    path.display()
                );
            }
            println!("PASS: every tile survives viewport uniform growth");
            robot.exit().expect("exit");
        })
        .run(UniformGrowthGrid);
}

#[allow(non_snake_case)]
#[composable]
fn UniformGrowthGrid() {
    let rows = rememberMutableStateOf(|| 1_u32);
    ROW_COUNT.with(|cell| cell.set(Some(rows)));
    Column(
        Modifier::empty().fill_max_size().background(Color::WHITE),
        ColumnSpec::default(),
        move || {
            for row in 0..rows.get() {
                Row(Modifier::empty(), RowSpec::default(), move || {
                    for column in 0..COLUMNS {
                        Box(
                            Modifier::empty()
                                .size(Size::new(TILE as f32, TILE as f32))
                                .graphics_layer(|| GraphicsLayer {
                                    compositing_strategy: CompositingStrategy::Offscreen,
                                    ..Default::default()
                                })
                                .draw_behind(move |scope| {
                                    scope.draw_rect(Brush::solid(Color::from_rgb_u8(
                                        INK[0] + column as u8,
                                        INK[1] + row as u8,
                                        INK[2],
                                    )));
                                }),
                            BoxSpec::default(),
                            || {},
                        );
                    }
                });
            }
        },
    );
}
