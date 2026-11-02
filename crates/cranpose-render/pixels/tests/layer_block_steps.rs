// A scene a shell updates in place, step after step, draws as a shell made
// at that step draws, for a parent whose layer block alone reads the step,
// also while another shell lives on the same thread.

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key, rememberMutableStateOf};
use cranpose_macros::composable;
use cranpose_render_pixels::{Picture, PixelsRenderer};
use cranpose_ui::{Box, BoxSpec, Modifier};
use cranpose_ui_graphics::{Color, GraphicsLayer};

const WIDTH: u32 = 120;
const HEIGHT: u32 = 80;
const STEPS: u32 = 6;

type Step = Rc<RefCell<Option<MutableState<u32>>>>;

#[composable]
fn FadingParent(start: u32, captured: Step) {
    let step = rememberMutableStateOf(move || start);
    *captured.borrow_mut() = Some(step);
    Box(
        Modifier::empty()
            .offset(20.0, 20.0)
            .width(80.0)
            .height(40.0)
            .graphics_layer(move || GraphicsLayer {
                alpha: 0.2 + (step.value() % 5) as f32 * 0.2,
                ..GraphicsLayer::default()
            })
            .background(Color::from_rgb_u8(30, 40, 50)),
        BoxSpec::default(),
        // A child that never changes takes the parent's alpha.
        || {
            Box(
                Modifier::empty()
                    .offset(10.0, 10.0)
                    .width(40.0)
                    .height(16.0)
                    .background(Color::from_rgb_u8(200, 120, 40)),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

fn shell(start: u32) -> (AppShell<PixelsRenderer>, Step) {
    let captured: Step = Rc::default();
    let content = Rc::clone(&captured);
    let shell = AppShell::new_with_size_and_density(
        PixelsRenderer::new(),
        location_key(file!(), line!(), column!()),
        move || FadingParent(start, Rc::clone(&content)),
        (WIDTH, HEIGHT),
        (WIDTH as f32, HEIGHT as f32),
        1.0,
    );
    (shell, captured)
}

fn draw(shell: &mut AppShell<PixelsRenderer>) -> Vec<u8> {
    let mut pixels = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    shell
        .primary()
        .with_renderer(|renderer| renderer.draw_scaled(&mut pixels, WIDTH, HEIGHT, 1.0));
    pixels
}

// Draws the areas the shell's picture changed in into `kept`.
fn redraw_changed(shell: &mut AppShell<PixelsRenderer>, picture: &mut Picture, kept: &mut [u8]) {
    let mut areas = Vec::new();
    let mut area_pixels = Vec::new();
    shell.primary().with_renderer(|renderer| {
        renderer.update_picture(picture, WIDTH, HEIGHT, 1.0, &mut areas);
        for area in &areas {
            area_pixels.resize(area.width as usize * area.height as usize * 4, 0);
            renderer.draw_picture_area(picture, *area, &mut area_pixels);
            let row_bytes = area.width as usize * 4;
            for (row, source) in area_pixels.chunks_exact(row_bytes).enumerate() {
                let start = ((area.y as usize + row) * WIDTH as usize + area.x as usize) * 4;
                kept[start..start + row_bytes].copy_from_slice(source);
            }
        }
    });
}

#[test]
fn each_step_a_layer_block_reads_paints_as_a_shell_made_at_that_step() {
    let (mut kept_shell, captured) = shell(0);
    let mut picture = Picture::default();
    let mut kept = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    kept_shell.update();
    redraw_changed(&mut kept_shell, &mut picture, &mut kept);
    for step in 1..STEPS {
        (*captured.borrow())
            .expect("the composition remembered the step")
            .set(step);
        kept_shell.update();
        redraw_changed(&mut kept_shell, &mut picture, &mut kept);
        let (mut made, _) = shell(step);
        made.update();
        let made = draw(&mut made);
        assert!(
            draw(&mut kept_shell) == made,
            "step {step}: the scene updated in place differs from one made at that step"
        );
        assert!(
            kept == made,
            "step {step}: the picture's changed areas leave a frame unlike one made at that step"
        );
    }
}

// Two windows on one thread: a change the first shell's layer block reads
// reaches it while a second shell lives.
#[test]
fn a_layer_block_change_reaches_its_shell_while_another_shell_lives() {
    let (mut first, captured) = shell(0);
    first.update();
    let (mut second, _) = shell(0);
    second.update();
    for step in 1..STEPS {
        (*captured.borrow())
            .expect("the composition remembered the step")
            .set(step);
        first.update();
        let (mut made, _) = shell(step);
        made.update();
        assert!(
            draw(&mut first) == draw(&mut made),
            "step {step}: the first shell missed a change while a second shell lives"
        );
    }
}
