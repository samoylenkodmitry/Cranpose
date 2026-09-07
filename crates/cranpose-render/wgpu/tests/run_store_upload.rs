mod support;

use cranpose_ui_graphics::Color;

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
