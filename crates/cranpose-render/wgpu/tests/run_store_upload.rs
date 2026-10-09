use cranpose_ui_graphics::{Brush, Color, Point};

use crate::support;

const WIDTH: u32 = 128;
const HEIGHT: u32 = 96;
const RECORDS: usize = 72;

const RED: Color = Color(0.8, 0.2, 0.2, 1.0);
const GREEN: Color = Color(0.2, 0.7, 0.3, 1.0);

fn pixel_at_record(pixels: &[u8], index: usize) -> [u8; 3] {
    let rect = support::stored_run_rect(index);
    let x = (rect.x + rect.width * 0.5) as usize;
    let y = (rect.y + rect.height * 0.5) as usize;
    let at = (y * WIDTH as usize + x) * 4;
    [pixels[at + 2], pixels[at + 1], pixels[at]]
}

fn is_red(pixel: [u8; 3]) -> bool {
    pixel[0] > 200 && pixel[1] < 160
}

fn is_green(pixel: [u8; 3]) -> bool {
    pixel[1] > 200 && pixel[0] < 160
}

fn background(pixels: &[u8]) -> [u8; 3] {
    let at = ((HEIGHT as usize - 1) * WIDTH as usize + WIDTH as usize - 1) * 4;
    [pixels[at + 2], pixels[at + 1], pixels[at]]
}

#[test]
fn a_stored_run_draws_every_record_the_recording_changed() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping run store upload: headless WGPU init failed: {err}");
            return;
        }
    };
    let present = |renderer: &mut support::LockedRenderer, colors: &[Color]| {
        support::present_and_read(
            renderer,
            WIDTH,
            HEIGHT,
            support::stored_run_graph(WIDTH, HEIGHT, colors),
        )
    };
    let all_red = vec![RED; RECORDS];
    let first = present(&mut renderer, &all_red);
    assert!(is_red(pixel_at_record(&first, 0)));
    assert!(is_red(pixel_at_record(&first, 50)));

    let mut one_green = all_red.clone();
    one_green[50] = GREEN;
    let middle = present(&mut renderer, &one_green);
    assert!(
        is_green(pixel_at_record(&middle, 50)),
        "the recoloured record in the middle of the table must draw green"
    );
    assert!(is_red(pixel_at_record(&middle, 0)));
    assert!(is_red(pixel_at_record(&middle, RECORDS - 1)));

    let mut last_green = all_red.clone();
    last_green[RECORDS - 1] = GREEN;
    let last = present(&mut renderer, &last_green);
    assert!(
        is_red(pixel_at_record(&last, 50)),
        "the middle record must be red again"
    );
    assert!(is_green(pixel_at_record(&last, RECORDS - 1)));

    let mut appended = all_red.clone();
    appended.push(GREEN);
    let longer = present(&mut renderer, &appended);
    assert!(
        is_green(pixel_at_record(&longer, RECORDS)),
        "a record appended past the old table's end must draw"
    );
    assert!(is_red(pixel_at_record(&longer, RECORDS - 1)));

    let shorter = present(&mut renderer, &all_red[..RECORDS - 1]);
    let background = background(&shorter);
    assert_eq!(
        pixel_at_record(&shorter, RECORDS - 1),
        background,
        "a record the recording dropped must not draw from the stale table"
    );
    assert_eq!(pixel_at_record(&shorter, RECORDS), background);
    assert!(is_red(pixel_at_record(&shorter, RECORDS - 2)));
}

/// Every frame recolours another record of a stored run, and some change
/// its length, while the frames before it may still be on the GPU: each
/// frame draws the tables it was recorded with, whether its changes go to a
/// version no frame in flight reads or queue as copies between the frames.
#[test]
fn stored_runs_changed_every_frame_draw_their_own_tables_while_earlier_frames_are_in_flight() {
    const FRAMES: usize = 8;
    type Renderer = fn() -> Result<support::LockedRenderer, String>;
    let renderers: [(&str, Renderer); 2] = [
        ("default uploads", support::headless_renderer),
        ("copied uploads", support::headless_renderer_copying_uploads),
    ];
    for (uploads, renderer) in renderers {
        let mut renderer = match renderer() {
            Ok(renderer) => renderer,
            Err(err) => {
                eprintln!("skipping run store upload: headless WGPU init failed: {err}");
                return;
            }
        };
        let green = |frame: usize| frame * 7 % RECORDS;
        let frames: Vec<Vec<Color>> = (0..FRAMES)
            .map(|frame| {
                let mut colors = vec![RED; RECORDS + frame % 3];
                colors[green(frame)] = GREEN;
                colors
            })
            .collect();
        let presented: Vec<wgpu::Texture> = frames
            .iter()
            .map(|colors| {
                support::present(
                    &mut renderer,
                    WIDTH,
                    HEIGHT,
                    support::stored_run_graph(WIDTH, HEIGHT, colors),
                )
            })
            .collect();
        for (frame, (colors, texture)) in frames.iter().zip(&presented).enumerate() {
            let pixels = support::read_presented(&renderer, texture);
            for index in 0..colors.len() {
                let pixel = pixel_at_record(&pixels, index);
                let expected = if index == green(frame) {
                    is_green(pixel)
                } else {
                    is_red(pixel)
                };
                assert!(
                    expected,
                    "{uploads}: frame {frame} must draw record {index} as recorded, drew {pixel:?}"
                );
            }
        }
    }
}

#[test]
fn stored_runs_that_take_gradients_after_solid_frames_paint_their_own() {
    const TALL: u32 = 160;
    const PER_RUN: usize = 64;
    const BLUE: Color = Color(0.2, 0.3, 0.9, 1.0);
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping run store upload: headless WGPU init failed: {err}");
            return;
        }
    };
    let present = |renderer: &mut support::LockedRenderer, runs: [Vec<Brush>; 2]| {
        support::present_and_read(
            renderer,
            WIDTH,
            TALL,
            support::stored_runs_graph_of(WIDTH, TALL, runs),
        )
    };
    let at = |pixels: &[u8], index: usize| {
        let rect = support::stored_run_rect(index);
        let x = (rect.x + rect.width * 0.5) as usize;
        let y = (rect.y + rect.height * 0.5) as usize;
        let at = (y * WIDTH as usize + x) * 4;
        [pixels[at + 2], pixels[at + 1], pixels[at]]
    };
    let gradient = |index: usize, color: Color| {
        let rect = support::stored_run_rect(index);
        Brush::linear_gradient_range(
            vec![color, color],
            Point::new(rect.x, rect.y),
            Point::new(rect.x + rect.width, rect.y),
        )
    };
    let solid = || vec![Brush::solid(RED); PER_RUN];
    // Solid records only: both runs bind the store's empty brush table.
    let first = present(&mut renderer, [solid(), solid()]);
    assert!(is_red(at(&first, 10)) && is_red(at(&first, PER_RUN + 10)));

    let mut green = solid();
    green[10] = gradient(10, GREEN);
    let mut blue = solid();
    blue[10] = gradient(PER_RUN + 10, BLUE);
    let second = present(&mut renderer, [green, blue]);
    assert!(
        is_green(at(&second, 10)),
        "the first run paints its gradient from a brush table of its own"
    );
    let second_blue = at(&second, PER_RUN + 10);
    assert!(
        second_blue[2] > 200 && second_blue[1] < 160,
        "the second run paints its own gradient, not the first's: {second_blue:?}"
    );
    assert!(is_red(at(&second, 0)) && is_red(at(&second, PER_RUN)));
}
