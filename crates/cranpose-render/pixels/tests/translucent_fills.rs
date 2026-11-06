// A translucent solid fill paints the bytes that blending each pixel alone
// gives. One fill keeps what it left over each byte it met before; a picture
// drawn a pixel at a time starts every fill afresh at each pixel, so the
// two pictures agree only when a kept byte equals the blend of that pixel.

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_macros::composable;
use cranpose_render_pixels::{PixelRect, PixelsRenderer};
use cranpose_ui::{Box, BoxSpec, Modifier};
use cranpose_ui_graphics::{Brush, Color, Rect};

const WIDTH: u32 = 96;
const HEIGHT: u32 = 64;

// A dithered gradient under two translucent boxes that overlap: the pixels
// under the boxes hold many bytes, in other combinations in each channel.
#[composable]
fn TranslucentOverGradient() {
    Box(
        Modifier::empty().fill_max_size().draw_behind(|scope| {
            let size = scope.size();
            scope.draw_rect_at(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: size.width,
                    height: size.height,
                },
                Brush::vertical_gradient(
                    vec![
                        Color::from_rgb_u8(250, 12, 90),
                        Color::from_rgb_u8(8, 200, 160),
                        Color::from_rgb_u8(120, 40, 255),
                    ],
                    0.0,
                    size.height,
                ),
            );
        }),
        BoxSpec::default(),
        || {
            Box(
                Modifier::empty()
                    .offset(6.0, 4.0)
                    .width(70.0)
                    .height(50.0)
                    .background(Color::from_rgba_u8(30, 200, 90, 94)),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .offset(30.0, 20.0)
                    .width(60.0)
                    .height(40.0)
                    .background(Color::from_rgba_u8(240, 120, 10, 207)),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

#[test]
fn a_translucent_fill_paints_the_bytes_each_pixel_blends_alone() {
    let mut shell = AppShell::new_with_size_and_density(
        PixelsRenderer::new(),
        location_key(file!(), line!(), column!()),
        TranslucentOverGradient,
        (WIDTH, HEIGHT),
        (WIDTH as f32, HEIGHT as f32),
        1.0,
    );
    shell.update();
    let mut whole = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    let mut by_pixel = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    shell.primary().with_renderer(|renderer| {
        renderer.draw(&mut whole, WIDTH, HEIGHT);
        renderer.update_picture(WIDTH, HEIGHT, &mut Vec::new());
        for (index, pixel) in by_pixel.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let area = PixelRect {
                x: index as u32 % WIDTH,
                y: index as u32 / WIDTH,
                width: 1,
                height: 1,
            };
            renderer.draw_picture_area(area, pixel);
        }
    });
    let differing = whole
        .as_chunks::<4>()
        .0
        .iter()
        .zip(by_pixel.as_chunks::<4>().0)
        .position(|(a, b)| a != b);
    assert_eq!(
        differing.map(|pixel| (pixel as u32 % WIDTH, pixel as u32 / WIDTH)),
        None,
        "the picture drawn whole differs from the one drawn a pixel at a time at (x, y)"
    );
}
