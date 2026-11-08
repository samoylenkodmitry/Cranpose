// A translucent solid fill paints the bytes that blending each pixel alone
// gives. One fill keeps what it left over each byte it met before; a picture
// drawn a pixel at a time starts every fill afresh at each pixel, so the
// two pictures agree only when a kept byte equals the blend of that pixel.

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_macros::composable;
use cranpose_render_pixels::{Picture, PixelRect, PixelsRenderer, Rgb565Order};
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

fn shell() -> AppShell<PixelsRenderer> {
    let mut shell = AppShell::new_with_size_and_density(
        PixelsRenderer::new(),
        location_key(file!(), line!(), column!()),
        TranslucentOverGradient,
        (WIDTH, HEIGHT),
        (WIDTH as f32, HEIGHT as f32),
        1.0,
    );
    shell.update();
    shell
}

// The pixel at `index` of a picture `WIDTH` wide, as an area.
fn pixel_area(index: usize) -> PixelRect {
    PixelRect {
        x: index as u32 % WIDTH,
        y: index as u32 / WIDTH,
        width: 1,
        height: 1,
    }
}

// Where two pictures first differ, as (x, y).
fn first_difference<T: PartialEq>(a: &[T], b: &[T]) -> Option<(u32, u32)> {
    let pixel = a.iter().zip(b).position(|(a, b)| a != b)?;
    Some((pixel as u32 % WIDTH, pixel as u32 / WIDTH))
}

#[test]
fn a_translucent_fill_paints_the_bytes_each_pixel_blends_alone() {
    let mut shell = shell();
    let mut whole = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    let mut by_pixel = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    let mut picture = Picture::default();
    shell.primary().with_renderer(|renderer| {
        renderer.draw(&mut whole, WIDTH, HEIGHT);
        renderer.update_picture(&mut picture, WIDTH, HEIGHT, 1.0, &mut Vec::new());
        for (index, pixel) in by_pixel.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            renderer.draw_picture_area(&picture, pixel_area(index), pixel);
        }
    });
    assert_eq!(
        first_difference(whole.as_chunks::<4>().0, by_pixel.as_chunks::<4>().0),
        None,
        "the picture drawn whole differs from the one drawn a pixel at a time at (x, y)"
    );
}

// The same for 16-bit pixels in either byte order, which blend in whole
// numbers and keep their high bits.
#[test]
fn a_translucent_fill_paints_the_16_bit_pixels_each_pixel_blends_alone() {
    let mut shell = shell();
    for order in [Rgb565Order::LittleEndian, Rgb565Order::BigEndian] {
        let mut whole = vec![0u16; (WIDTH * HEIGHT) as usize];
        let mut by_pixel = vec![0u16; (WIDTH * HEIGHT) as usize];
        let mut picture = Picture::default();
        shell.primary().with_renderer(|renderer| {
            renderer.update_picture(&mut picture, WIDTH, HEIGHT, 1.0, &mut Vec::new());
            let frame = PixelRect {
                x: 0,
                y: 0,
                width: WIDTH,
                height: HEIGHT,
            };
            renderer.draw_picture_area_rgb565(&picture, frame, &mut whole, order);
            for (index, pixel) in by_pixel.iter_mut().enumerate() {
                renderer.draw_picture_area_rgb565(
                    &picture,
                    pixel_area(index),
                    std::slice::from_mut(pixel),
                    order,
                );
            }
        });
        assert_eq!(
            first_difference(&whole, &by_pixel),
            None,
            "{order:?}: the picture drawn whole differs from the one drawn a pixel at a time at (x, y)"
        );
    }
}
