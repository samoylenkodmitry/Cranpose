// The rows of a fill in every pixel format the renderer draws: a clip that
// cuts a fill's rows leaves the pixels the fill paints inside it, a fill no
// pixel wide paints nothing, one a pixel wide paints its one column, and a
// fill that reaches the last pixel of the buffer paints it.

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_macros::composable;
use cranpose_render_pixels::{Picture, PixelRect, PixelsRenderer, Rgb565Order};
use cranpose_ui::{Box, BoxSpec, Modifier, Size};
use cranpose_ui_graphics::{Brush, Color, CornerRadii, Rect};

const WIDTH: u32 = 64;
const HEIGHT: u32 = 48;

const BACKGROUND: Color = Color::from_rgb_u8(20, 30, 40);
const SOLID: Color = Color::from_rgb_u8(200, 120, 40);
const TOP: Color = Color::from_rgb_u8(76, 201, 240);
const BOTTOM: Color = Color::from_rgb_u8(67, 97, 238);

// The box that clips the fills of `Clipped`, in pixels.
const CLIP: PixelRect = PixelRect {
    x: 16,
    y: 8,
    width: 32,
    height: 32,
};

fn gradient() -> Brush {
    Brush::vertical_gradient(vec![TOP, BOTTOM], 0.0, f32::INFINITY)
}

fn frame() -> PixelRect {
    PixelRect {
        x: 0,
        y: 0,
        width: WIDTH,
        height: HEIGHT,
    }
}

// Fills that reach past the clip box on both sides: a rounded fill whose
// middle rows start at the box's top and whose lower corners lie inside it,
// and a rounded gradient off the pixel grid. With `clipped` false the box
// does not clip them.
#[composable]
fn Clipped(clipped: bool) {
    Box(
        Modifier::empty().fill_max_size().background(BACKGROUND),
        BoxSpec::default(),
        move || {
            let place = Modifier::empty()
                .offset(CLIP.x as f32, CLIP.y as f32)
                .size(Size::new(CLIP.width as f32, CLIP.height as f32));
            let place = if clipped {
                place.clip_to_bounds()
            } else {
                place
            };
            Box(place, BoxSpec::default(), || {
                Box(
                    Modifier::empty().fill_max_size().draw_behind(|scope| {
                        scope.draw_round_rect_at(
                            Rect {
                                x: -9.0,
                                y: -6.0,
                                width: 48.0,
                                height: 20.0,
                            },
                            Brush::solid(SOLID),
                            CornerRadii::uniform(6.0),
                        );
                        scope.draw_round_rect_at(
                            Rect {
                                x: -5.5,
                                y: 16.25,
                                width: 43.0,
                                height: 14.0,
                            },
                            gradient(),
                            CornerRadii::uniform(3.0),
                        );
                    }),
                    BoxSpec::default(),
                    || {},
                );
            });
        },
    );
}

// Columns of `Narrow`: fills no pixel wide and one pixel wide, solid and of a
// gradient, and a gradient as wide as eight pixels whose first column lies as
// far into the dither pattern as the narrow gradient's column does.
const EMPTY_SOLID: u32 = 4;
const ONE_SOLID: u32 = 7;
const ONE_GRADIENT: u32 = 10;
const WIDE_GRADIENT: u32 = 18;
const EMPTY_GRADIENT: u32 = 30;
// The rows the narrow fills cover.
const NARROW_ROWS: std::ops::Range<u32> = 4..20;

#[composable]
fn Narrow() {
    Box(
        Modifier::empty().fill_max_size().background(BACKGROUND),
        BoxSpec::default(),
        || {
            Box(
                Modifier::empty().fill_max_size().draw_behind(|scope| {
                    let column = |x: u32, width: f32| Rect {
                        x: x as f32,
                        y: NARROW_ROWS.start as f32,
                        width,
                        height: NARROW_ROWS.len() as f32,
                    };
                    scope.draw_rect_at(column(EMPTY_SOLID, 0.0), Brush::solid(SOLID));
                    scope.draw_rect_at(column(ONE_SOLID, 1.0), Brush::solid(SOLID));
                    scope.draw_rect_at(column(ONE_GRADIENT, 1.0), gradient());
                    scope.draw_rect_at(column(WIDE_GRADIENT, 8.0), gradient());
                    scope.draw_round_rect_at(
                        column(EMPTY_GRADIENT, 0.0),
                        gradient(),
                        CornerRadii::uniform(2.0),
                    );
                    // A fill whose last row ends at the last pixel of the
                    // picture, and so of the buffer that holds it whole.
                    scope.draw_round_rect_at(
                        Rect {
                            x: (WIDTH - 12) as f32,
                            y: (HEIGHT - 9) as f32,
                            width: 12.0,
                            height: 9.0,
                        },
                        gradient(),
                        CornerRadii::uniform(0.0),
                    );
                }),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

fn shell(content: impl FnMut() + 'static) -> AppShell<PixelsRenderer> {
    let mut shell = AppShell::new_with_size_and_density(
        PixelsRenderer::new(),
        location_key(file!(), line!(), column!()),
        content,
        (WIDTH, HEIGHT),
        (WIDTH as f32, HEIGHT as f32),
        1.0,
    );
    shell.update();
    shell
}

#[derive(Clone, Copy, Debug)]
enum Format {
    Rgba,
    Rgb565(Rgb565Order),
}

const FORMATS: [Format; 3] = [
    Format::Rgba,
    Format::Rgb565(Rgb565Order::LittleEndian),
    Format::Rgb565(Rgb565Order::BigEndian),
];

// The pixels of `area` of the picture in `format`, a value a pixel: the
// RGBA bytes in native order, or the 16-bit pixel as stored.
fn draw(renderer: &PixelsRenderer, picture: &Picture, format: Format, area: PixelRect) -> Vec<u32> {
    let len = area.width as usize * area.height as usize;
    match format {
        Format::Rgba => {
            let mut bytes = vec![0u8; len * 4];
            renderer.draw_picture_area(picture, area, &mut bytes);
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|&pixel| u32::from_ne_bytes(pixel))
                .collect()
        }
        Format::Rgb565(order) => {
            let mut words = vec![0u16; len];
            renderer.draw_picture_area_rgb565(picture, area, &mut words, order);
            words.into_iter().map(u32::from).collect()
        }
    }
}

// The whole picture of `shell` in `format`, drawn at once, and drawn again a
// pixel at a time into the same places.
fn pictures(shell: &mut AppShell<PixelsRenderer>, format: Format) -> (Vec<u32>, Vec<u32>) {
    shell.primary().with_renderer(|renderer| {
        let mut picture = Picture::default();
        renderer.update_picture(&mut picture, WIDTH, HEIGHT, 1.0, &mut Vec::new());
        let whole = draw(renderer, &picture, format, frame());
        let by_pixel = (0..HEIGHT)
            .flat_map(|y| (0..WIDTH).map(move |x| (x, y)))
            .map(|(x, y)| {
                let area = PixelRect {
                    x,
                    y,
                    width: 1,
                    height: 1,
                };
                draw(renderer, &picture, format, area)[0]
            })
            .collect();
        (whole, by_pixel)
    })
}

// The value `color` takes as an opaque pixel of `format`.
fn opaque(color: Color, format: Format) -> u32 {
    let [red, green, blue] =
        [color.0, color.1, color.2].map(|channel| (channel * 255.0).round() as u8);
    match format {
        Format::Rgba => u32::from_ne_bytes([red, green, blue, 255]),
        Format::Rgb565(order) => {
            let packed =
                (u16::from(red >> 3) << 11) | (u16::from(green >> 2) << 5) | u16::from(blue >> 3);
            u32::from(match order {
                Rgb565Order::LittleEndian => packed.to_le(),
                Rgb565Order::BigEndian => packed.to_be(),
            })
        }
    }
}

fn at(pixels: &[u32], x: u32, y: u32) -> u32 {
    pixels[(y * WIDTH + x) as usize]
}

fn inside_clip(x: u32, y: u32) -> bool {
    (CLIP.x..CLIP.x + CLIP.width).contains(&x) && (CLIP.y..CLIP.y + CLIP.height).contains(&y)
}

#[test]
fn a_clip_that_cuts_a_fill_s_rows_keeps_the_pixels_the_fill_paints_inside_it() {
    let mut clipped = shell(|| Clipped(true));
    let mut open = shell(|| Clipped(false));
    for format in FORMATS {
        let (clipped_whole, clipped_by_pixel) = pictures(&mut clipped, format);
        let (open_whole, _) = pictures(&mut open, format);
        assert_eq!(
            clipped_whole, clipped_by_pixel,
            "{format:?}: the clipped fills drawn whole differ from them drawn a pixel at a time"
        );
        let background = opaque(BACKGROUND, format);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let expected = if inside_clip(x, y) {
                    at(&open_whole, x, y)
                } else {
                    background
                };
                assert_eq!(
                    at(&clipped_whole, x, y),
                    expected,
                    "{format:?}: pixel ({x}, {y}) of the clipped fills"
                );
            }
        }
        // The fills reach both sides of the clip in the rows they cover.
        for (x, y) in [
            (CLIP.x, 10),
            (CLIP.x + CLIP.width - 1, 10),
            (CLIP.x, 30),
            (CLIP.x + CLIP.width - 1, 30),
        ] {
            assert_ne!(
                at(&clipped_whole, x, y),
                background,
                "{format:?}: the fill covers ({x}, {y}) at the clip's edge"
            );
        }
    }
}

#[test]
fn a_fill_no_pixel_wide_paints_nothing_and_one_a_pixel_wide_paints_its_column() {
    let mut narrow = shell(Narrow);
    for format in FORMATS {
        let (whole, by_pixel) = pictures(&mut narrow, format);
        assert_eq!(
            whole, by_pixel,
            "{format:?}: the narrow fills drawn whole differ from them drawn a pixel at a time"
        );
        let background = opaque(BACKGROUND, format);
        let solid = opaque(SOLID, format);
        for y in NARROW_ROWS {
            for x in [
                EMPTY_SOLID,
                ONE_SOLID - 1,
                ONE_SOLID + 1,
                ONE_GRADIENT + 1,
                EMPTY_GRADIENT,
            ] {
                assert_eq!(
                    at(&whole, x, y),
                    background,
                    "{format:?}: pixel ({x}, {y}) stays clear"
                );
            }
            assert_eq!(
                at(&whole, ONE_SOLID, y),
                solid,
                "{format:?}: the solid column at row {y}"
            );
            assert_eq!(
                at(&whole, ONE_GRADIENT, y),
                at(&whole, WIDE_GRADIENT, y),
                "{format:?}: the one-pixel gradient at row {y} takes the wide gradient's color"
            );
        }
    }
}

#[test]
fn a_fill_that_ends_at_the_buffer_s_last_pixel_paints_it() {
    let mut narrow = shell(Narrow);
    for format in FORMATS {
        let (whole, _) = pictures(&mut narrow, format);
        let last = whole.len() - 1;
        assert_ne!(
            whole[last],
            opaque(BACKGROUND, format),
            "{format:?}: the fill paints the last pixel"
        );
        // The last pixel alone, into a buffer of one pixel.
        let alone = narrow.primary().with_renderer(|renderer| {
            let mut picture = Picture::default();
            renderer.update_picture(&mut picture, WIDTH, HEIGHT, 1.0, &mut Vec::new());
            let corner = PixelRect {
                x: WIDTH - 1,
                y: HEIGHT - 1,
                width: 1,
                height: 1,
            };
            draw(renderer, &picture, format, corner)
        });
        assert_eq!(
            alone,
            [whole[last]],
            "{format:?}: the last pixel drawn alone"
        );
        // The last row alone, into a buffer of one row.
        let row = narrow.primary().with_renderer(|renderer| {
            let mut picture = Picture::default();
            renderer.update_picture(&mut picture, WIDTH, HEIGHT, 1.0, &mut Vec::new());
            let bottom = PixelRect {
                x: 0,
                y: HEIGHT - 1,
                width: WIDTH,
                height: 1,
            };
            draw(renderer, &picture, format, bottom)
        });
        assert_eq!(
            row,
            whole[whole.len() - WIDTH as usize..],
            "{format:?}: the last row drawn alone"
        );
    }
}
