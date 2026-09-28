//! A Liquid widget takes its caller's modifier first, as a Compose widget
//! does: an offset there moves the whole widget, its own drawing included.

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_liquid::prelude::*;
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui::{
    Color, Modifier, Size,
    widgets::{Box, BoxSpec},
};

use crate::support;

const WIDTH: u32 = 240;
const HEIGHT: u32 = 120;

/// `widget` over a black frame, drawn once it settled.
fn drawn(widget: impl Fn() + 'static) -> Option<CapturedFrame> {
    let (_lock, renderer) = support::headless_renderer_parts().ok()?;
    let widget = std::rc::Rc::new(widget);
    let mut shell = AppShell::new(
        renderer,
        location_key(file!(), line!(), column!()),
        move || {
            let widget = std::rc::Rc::clone(&widget);
            LiquidTheme(LiquidThemeSpec::default(), &mut move || {
                let widget = std::rc::Rc::clone(&widget);
                Box(
                    Modifier::empty().fill_max_size().background(Color::BLACK),
                    BoxSpec::default(),
                    move || widget(),
                );
            });
        },
    );
    shell.set_viewport(WIDTH as f32, HEIGHT as f32);
    shell.set_buffer_size(WIDTH, HEIGHT);
    Some(support::settle(|| {
        support::update_and_capture(&mut shell, WIDTH, HEIGHT)
    }))
}

fn rgb(frame: &CapturedFrame, x: u32, y: u32) -> (u8, u8, u8) {
    let index = ((y * frame.width + x) * 4) as usize;
    (
        frame.pixels[index],
        frame.pixels[index + 1],
        frame.pixels[index + 2],
    )
}

fn is_black((r, g, b): (u8, u8, u8)) -> bool {
    r < 30 && g < 30 && b < 30
}

#[test]
fn an_offset_surface_draws_its_fill_where_the_offset_puts_it() {
    let Some(frame) = drawn(|| {
        Surface(
            Modifier::empty()
                .offset(20.0, 20.0)
                .size(Size::new(80.0, 40.0)),
            || {},
        );
    }) else {
        return;
    };
    let (r, g, b) = rgb(&frame, 95, 55);
    assert!(
        r > 200 && g > 200 && b > 200,
        "the fill is at the offset: {:?}",
        (r, g, b)
    );
    assert!(
        is_black(rgb(&frame, 5, 5)),
        "and not where the node started"
    );
}

#[test]
fn an_offset_toggle_draws_its_track_where_the_offset_puts_it() {
    let Some(frame) = drawn(|| {
        LiquidToggle(Modifier::empty().offset(140.0, 60.0), true, |_| {});
    }) else {
        return;
    };
    let (r, _, b) = rgb(&frame, 148, 74);
    assert!(
        b > 150 && b > r,
        "the checked track is at the offset: {:?}",
        rgb(&frame, 148, 74)
    );
    assert!(
        is_black(rgb(&frame, 8, 14)),
        "and not where the node started"
    );
}
