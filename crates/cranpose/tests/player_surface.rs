use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use cranpose::{
    AccessUnit, AppLauncher, CropRect, DecodedFrames, DecoderConfig, DecoderOutput, PlayerSurface,
    PlayerSurfaceIntake, VideoCodec, VideoDecoder, embedded_view::EmbeddedView, prelude::*,
    split_access_units,
};

use crate::gpu_support::{draw_settled, pixel};

const H264_CLIP: &[u8] = include_bytes!("../../../apps/xr-demo/assets/test_clip.h264");
const HEVC_CLIP: &[u8] = include_bytes!("../../../apps/xr-demo/assets/test_clip.h265");
// Five bands of mid tones in the video range with the BT.601 matrix, and in the full range with
// the BT.709 matrix. A surface that reads one as the other shows other values: the saturated
// bars of the clips above hide the difference.
const TONES_601_LIMITED: &[u8] = include_bytes!("assets/test_tones_601_limited.h264");
const TONES_709_FULL: &[u8] = include_bytes!("assets/test_tones_709_full.h264");
// The middle of each band, with its color
const TONES: [((u32, u32), [u8; 3]); 5] = [
    ((64, 180), [64, 64, 64]),
    ((192, 180), [128, 128, 128]),
    ((320, 180), [192, 192, 192]),
    ((448, 180), [200, 100, 50]),
    ((576, 180), [50, 100, 200]),
];
const STEP: Duration = Duration::from_micros(16_667);
// The clip's size, which the view has as well
const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;

// The color bars of the clip, as the pictures show them: red, green, yellow, blue, magenta and
// cyan, from the left. A point in each, below the checkered patch and the diagonal.
const BARS: [((u32, u32), [u8; 3]); 6] = [
    ((50, 330), [255, 0, 0]),
    ((160, 330), [0, 255, 0]),
    ((270, 330), [255, 255, 0]),
    ((370, 330), [0, 0, 255]),
    ((480, 330), [255, 0, 255]),
    ((600, 330), [0, 255, 255]),
];

struct Player {
    decoder: VideoDecoder,
    view: EmbeddedView,
    width: u32,
    pixels: Vec<u8>,
    units: Vec<&'static [u8]>,
    fed: u64,
}

impl Player {
    fn new(codec: VideoCodec, clip: &'static [u8], content_scale: ContentScale) -> Self {
        Self::in_view(codec, clip, content_scale, (WIDTH, HEIGHT))
    }

    // A player whose view is `size` pixels, which need not be the picture's size.
    fn in_view(
        codec: VideoCodec,
        clip: &'static [u8],
        content_scale: ContentScale,
        size: (u32, u32),
    ) -> Self {
        Self::with_surface(codec, clip, size, move |frames| {
            PlayerSurface(frames, Modifier::empty().fill_max_size(), content_scale);
        })
    }

    // A player whose view is `size` pixels and shows the surface that `surface` composes for the
    // decoder's frames.
    fn with_surface(
        codec: VideoCodec,
        clip: &'static [u8],
        size: (u32, u32),
        surface: impl Fn(DecodedFrames) + 'static,
    ) -> Self {
        let decoder = VideoDecoder::new(DecoderConfig::new(
            codec,
            WIDTH,
            HEIGHT,
            DecoderOutput::Frames { depth: 6 },
        ))
        .expect("a decoder");
        let frames: DecodedFrames = decoder.frames();
        let mut view = AppLauncher::new()
            .create_embedded_view(size.0, size.1, 1.0, move || surface(frames.clone()))
            .expect("GPU embedded component");
        let mut pixels = Vec::new();
        draw_settled(&mut view, &mut pixels);
        Self {
            decoder,
            view,
            width: size.0,
            pixels,
            units: split_access_units(codec, clip),
            fed: 0,
        }
    }

    // Feeds the next picture and waits until the decoder outputs it.
    fn feed(&mut self) {
        let before = self.decoder.frames().stats().decoded;
        let unit = self.units[usize::try_from(self.fed).expect("small") % self.units.len()];
        self.decoder
            .submit(AccessUnit {
                data: unit,
                timestamp: STEP * u32::try_from(self.fed).expect("small"),
                metadata: self.fed,
            })
            .expect("a unit");
        self.fed += 1;
        let frames = self.decoder.frames();
        assert!(
            wait_until(|| frames.stats().decoded > before),
            "the decoder output no picture"
        );
    }

    // Feeds the next picture and draws until it shows, which the decoder's wake-up and the
    // surface's task do. Returns the time from the unit to the drawn picture.
    fn show_next(&mut self) -> Duration {
        let started = Instant::now();
        self.feed();
        let drawn = self.view.draw(&mut self.pixels).expect("frame");
        assert!(drawn, "the picture's frame drew nothing");
        draw_settled(&mut self.view, &mut self.pixels);
        started.elapsed()
    }

    fn color(&self, x: u32, y: u32) -> [u8; 3] {
        let pixel = pixel(&self.pixels, self.width, x, y);
        [pixel[0], pixel[1], pixel[2]]
    }
}

// Polls `done` until it is true, for five seconds at most, and tells whether it became true.
#[expect(
    clippy::disallowed_methods,
    reason = "a test waits for a thread of the decoder, and a poll is the way to see its counts"
)]
fn wait_until(done: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_micros(200));
    }
    true
}

// Checks that each point of `points` has its color, within `tolerance`.
fn assert_points(player: &Player, what: &str, points: &[((u32, u32), [u8; 3])], tolerance: u8) {
    for ((x, y), expected) in points {
        let color = player.color(*x, *y);
        let close = color
            .iter()
            .zip(expected)
            .all(|(have, want)| have.abs_diff(*want) <= tolerance);
        assert!(close, "{what}: ({x}, {y}) is {color:?}, not {expected:?}");
    }
}

fn assert_bars(player: &Player, what: &str) {
    assert_points(player, what, &BARS, 12);
}

// The clock's box is in the top left corner: dark orange, which a swapped channel or a flipped
// picture would not show there.
fn assert_clock(player: &Player, what: &str) {
    let [red, green, blue] = player.color(20, 14);
    assert!(
        red > green && green > blue && red < 160,
        "{what}: the top left corner is {:?}",
        [red, green, blue]
    );
}

#[test]
fn a_surface_draws_the_color_bars_of_each_clip() {
    for (codec, clip) in [(VideoCodec::H264, H264_CLIP), (VideoCodec::Hevc, HEVC_CLIP)] {
        let mut player = Player::new(codec, clip, ContentScale::FillBounds);
        // Before a picture, the surface draws the window's background
        let blank = player.color(50, 330);
        assert!(blank.iter().all(|channel| *channel < 40), "{blank:?}");
        player.show_next();
        assert_bars(&player, &format!("{codec:?} first picture"));
        assert_clock(&player, &format!("{codec:?} first picture"));
        for _ in 0..30 {
            player.show_next();
        }
        assert_bars(&player, &format!("{codec:?} later picture"));
        assert_clock(&player, &format!("{codec:?} later picture"));
    }
}

#[test]
fn a_surface_follows_the_range_and_the_matrix_of_the_stream() {
    for (clip, what) in [
        (TONES_601_LIMITED, "a video range BT.601 stream"),
        (TONES_709_FULL, "a full range BT.709 stream"),
    ] {
        let mut player = Player::new(VideoCodec::H264, clip, ContentScale::FillBounds);
        for _ in 0..5 {
            player.show_next();
        }
        // The bands are flat, so only the conversion can move them
        assert_points(&player, what, &TONES, 4);
    }
}

#[test]
fn a_surface_fits_or_crops_the_picture_to_its_box() {
    // Fit: the picture is as wide as the view and centered, with 140 rows of background above
    let mut player = Player::in_view(VideoCodec::H264, H264_CLIP, ContentScale::Fit, (WIDTH, 640));
    player.show_next();
    assert_points(
        &player,
        "a fitted picture",
        &[
            ((50, 140 + 330), [255, 0, 0]),
            ((600, 140 + 330), [0, 255, 255]),
        ],
        12,
    );
    let background = player.color(50, 40);
    assert!(
        background.iter().all(|channel| *channel < 40),
        "{background:?}"
    );
    // Crop: the view shows the middle 360 columns of the picture, from column 140
    let mut player = Player::in_view(VideoCodec::H264, H264_CLIP, ContentScale::Crop, (360, 360));
    player.show_next();
    assert_points(
        &player,
        "a cropped picture",
        &[
            ((30, 330), [0, 255, 0]),
            ((120, 330), [255, 255, 0]),
            ((220, 330), [0, 0, 255]),
            ((330, 330), [255, 0, 255]),
        ],
        12,
    );
}

#[test]
fn a_surface_shows_the_part_that_its_intake_presents() {
    // The intake presents the right half of each picture, and tells whether the surface took it
    let results = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&results);
    let mut player = Player::with_surface(VideoCodec::H264, H264_CLIP, (320, 360), move |frames| {
        let log = Rc::clone(&log);
        PlayerSurfaceIntake(
            frames,
            Modifier::empty().fill_max_size(),
            ContentScale::Fit,
            move |frames, presenter| {
                while let Some(frame) = frames.next_frame() {
                    let right_half = CropRect {
                        x: 320,
                        y: 0,
                        width: 320,
                        height: 360,
                    };
                    log.borrow_mut()
                        .push(presenter.present(frame, Some(right_half)));
                }
            },
        );
    });
    player.show_next();
    assert_points(
        &player,
        "the right half",
        &[
            ((50, 330), [0, 0, 255]),
            ((160, 330), [255, 0, 255]),
            ((270, 330), [0, 255, 255]),
        ],
        12,
    );
    assert_eq!(*results.borrow(), [true]);
}

#[test]
fn a_surface_refuses_a_part_that_reaches_outside_the_picture() {
    let results = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&results);
    let mut player = Player::with_surface(
        VideoCodec::H264,
        H264_CLIP,
        (WIDTH, HEIGHT),
        move |frames| {
            let log = Rc::clone(&log);
            PlayerSurfaceIntake(
                frames,
                Modifier::empty().fill_max_size(),
                ContentScale::Fit,
                move |frames, presenter| {
                    while let Some(frame) = frames.next_frame() {
                        // The part starts inside and ends 10 columns past the picture's edge
                        let part = CropRect {
                            x: 330,
                            y: 0,
                            width: 320,
                            height: 360,
                        };
                        log.borrow_mut().push(presenter.present(frame, Some(part)));
                    }
                },
            );
        },
    );
    player.feed();
    // The surface draws nothing, so the frame that takes the picture has nothing to show
    assert!(!player.view.draw(&mut player.pixels).expect("frame"));
    assert_eq!(*results.borrow(), [false]);
}
