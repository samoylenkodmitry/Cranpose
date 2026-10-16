//! The shared case for `DrawScope` text drawn with a gradient brush.

use std::rc::Rc;

use cranpose_ui::DrawCommand;
use cranpose_ui_graphics::{Brush, Color, DrawScope, DrawScopeDefault, DrawTextStyle, Size};

use super::{RenderFixture, build_fixture, is_background_like};
use crate::{
    graph::PrimitivePhase, image_compare::sample_pixel,
    scene_builder::draw_command_nodes_for_tests, style_shared::DrawPlacement,
};

const WIDTH: u32 = 200;
const HEIGHT: u32 = 60;

fn gradient_text(scope: &mut DrawScopeDefault) {
    scope.draw_text(
        Brush::horizontal_gradient_default(vec![Color::RED, Color::BLUE]),
        "HHHHHH",
        &DrawTextStyle::new(40.0),
    );
}

pub(super) fn gradient_text_fixture() -> RenderFixture {
    let nodes = draw_command_nodes_for_tests(
        1,
        &[DrawCommand::Behind(Rc::new(gradient_text))],
        DrawPlacement::Behind,
        Size::new(WIDTH as f32, HEIGHT as f32),
        PrimitivePhase::BeforeChildren,
    );
    build_fixture(WIDTH, HEIGHT, nodes)
}

/// The summed red and blue of the ink in `columns`.
fn ink_red_blue(pixels: &[u8], columns: std::ops::Range<u32>, background: [u8; 4]) -> (u32, u32) {
    let mut sums = (0, 0);
    for x in columns {
        for y in 0..HEIGHT {
            let pixel = sample_pixel(pixels, WIDTH, x, y);
            if !is_background_like(pixel, background) {
                sums.0 += u32::from(pixel[0]);
                sums.1 += u32::from(pixel[2]);
            }
        }
    }
    sums
}

pub(super) fn assert_gradient_text_frame(pixels: &[u8], width: u32, height: u32) {
    assert_eq!((width, height), (WIDTH, HEIGHT));
    let background = sample_pixel(pixels, width, width - 2, height - 2);
    let ink: Vec<u32> = (0..WIDTH)
        .filter(|&x| {
            (0..HEIGHT).any(|y| !is_background_like(sample_pixel(pixels, WIDTH, x, y), background))
        })
        .collect();
    let (Some(&left), Some(&right)) = (ink.first(), ink.last()) else {
        panic!("the gradient text draws no ink");
    };
    let fifth = (right - left) / 5;
    assert!(fifth > 4, "the text spans {left}..{right}");
    let (red, blue) = ink_red_blue(pixels, left..left + fifth, background);
    assert!(
        red > blue * 2,
        "the start of the text is red: red {red}, blue {blue}"
    );
    let (red, blue) = ink_red_blue(pixels, right - fifth..right + 1, background);
    assert!(
        blue > red * 2,
        "the end of the text is blue: red {red}, blue {blue}"
    );
}
