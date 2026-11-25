use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use cranpose::{
    AccessUnit, AppLauncher, CropRect, DecodeOrder, DecodedFrames, DecoderConfig, DecoderOutput,
    PlayerSurface, PlayerSurfaceIntake, PresentError, VideoCodec, VideoDecoder, VideoRotation,
    embedded_view::EmbeddedView, prelude::*, split_access_units, video_surface_supported,
};

use crate::gpu_support::{draw_settled, pixel};

const H264_CLIP: &[u8] = include_bytes!("../../../apps/xr-demo/assets/test_clip.h264");
const HEVC_CLIP: &[u8] = include_bytes!("../../../apps/xr-demo/assets/test_clip.h265");
// Five bands of mid tones in the video range with the BT.601 matrix, and in the full range with
// the BT.709 matrix. A surface that reads one as the other shows other values: the saturated
// bars of the clips above hide the difference.
const TONES_601_LIMITED: &[u8] = include_bytes!("assets/test_tones_601_limited.h264");
const TONES_709_FULL: &[u8] = include_bytes!("assets/test_tones_709_full.h264");
const TONES_2020_PQ: &[u8] = include_bytes!("assets/test_tones_2020_pq.h265");
const TONES_2020_HLG: &[u8] = include_bytes!("assets/test_tones_2020_hlg.h265");
const BFRAMES_H264: &[u8] = include_bytes!("assets/test_bframes.h264");
const BFRAMES_H264_PTS: [u64; 12] = [0, 4, 2, 1, 3, 8, 6, 5, 7, 11, 9, 10];
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
    assert_eq!(*results.borrow(), [Ok(())]);
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
    assert_eq!(*results.borrow(), [Err(PresentError::PartOutside)]);
}

#[test]
fn a_picture_that_the_intake_leaves_reaches_its_next_call() {
    // The first call takes nothing. The surface must still yield to the window, and the next
    // call must get the picture.
    let calls = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&calls);
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
                move |frames, _presenter| {
                    let first = log.borrow().is_empty();
                    let taken = if first { None } else { frames.next_frame() };
                    log.borrow_mut()
                        .push(taken.map(|frame| frame.info().metadata));
                },
            );
        },
    );
    player.feed();
    let deadline = Instant::now() + Duration::from_secs(5);
    while calls.borrow().len() < 2 && Instant::now() < deadline {
        player.view.draw(&mut player.pixels).expect("frame");
    }
    assert_eq!(*calls.borrow(), [None, Some(0)]);
    // With no picture left, the surface asks for no more frames
    draw_settled(&mut player.view, &mut player.pixels);
    assert_eq!(calls.borrow().len(), 2);
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "a test draws until the decoder's second picture arrives, and a poll is the way to see it"
)]
fn a_picture_that_comes_while_the_intake_runs_reaches_its_next_call() {
    // The intake takes the first picture. While it runs, the decoder outputs the second, as a
    // stream's next picture comes while the surface converts the one before it.
    let decoder = Rc::new(RefCell::new(
        VideoDecoder::new(DecoderConfig::new(
            VideoCodec::H264,
            WIDTH,
            HEIGHT,
            DecoderOutput::Frames { depth: 6 },
        ))
        .expect("a decoder"),
    ));
    let units = split_access_units(VideoCodec::H264, H264_CLIP);
    let submit = {
        let decoder = Rc::clone(&decoder);
        move |index: usize| {
            let before = decoder.borrow().frames().stats().decoded;
            decoder
                .borrow_mut()
                .submit(AccessUnit {
                    data: units[index],
                    timestamp: STEP * u32::try_from(index).expect("small"),
                    metadata: u64::try_from(index).expect("small"),
                })
                .expect("a unit");
            let frames = decoder.borrow().frames();
            assert!(
                wait_until(|| frames.stats().decoded > before),
                "the decoder output no picture"
            );
        }
    };
    let submit = Rc::new(submit);
    let taken = Rc::new(RefCell::new(Vec::new()));
    let frames = decoder.borrow().frames();
    let mut view = AppLauncher::new()
        .create_embedded_view(WIDTH, HEIGHT, 1.0, {
            let (submit, taken) = (Rc::clone(&submit), Rc::clone(&taken));
            move || {
                let (submit, taken) = (Rc::clone(&submit), Rc::clone(&taken));
                PlayerSurfaceIntake(
                    frames.clone(),
                    Modifier::empty().fill_max_size(),
                    ContentScale::Fit,
                    move |frames, presenter| {
                        while let Some(frame) = frames.next_frame() {
                            taken.borrow_mut().push(frame.info().metadata);
                            presenter.present(frame, None).expect("a shown picture");
                        }
                        if taken.borrow().len() == 1 {
                            submit(1);
                        }
                    },
                );
            }
        })
        .expect("GPU embedded component");
    let mut pixels = Vec::new();
    draw_settled(&mut view, &mut pixels);
    submit(0);
    let deadline = Instant::now() + Duration::from_secs(5);
    while taken.borrow().len() < 2 && Instant::now() < deadline {
        view.draw(&mut pixels).expect("frame");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(*taken.borrow(), [0, 1]);
}

#[test]
fn a_surface_rotates_pictures_clockwise_and_swaps_axes() {
    assert!(video_surface_supported());
    for (rotation, view_size, clock_at, red_bar_at, cyan_bar_at) in [
        // 90 deg CW: source (x, y) -> target (360 - 1 - y, x)
        (
            VideoRotation::Degrees90,
            (HEIGHT, WIDTH),
            (HEIGHT - 1 - 14, 20),
            (HEIGHT - 1 - 330, 50),
            (HEIGHT - 1 - 330, 600),
        ),
        // 180 deg: source (x, y) -> target (640 - 1 - x, 360 - 1 - y)
        (
            VideoRotation::Degrees180,
            (WIDTH, HEIGHT),
            (WIDTH - 1 - 20, HEIGHT - 1 - 14),
            (WIDTH - 1 - 50, HEIGHT - 1 - 330),
            (WIDTH - 1 - 600, HEIGHT - 1 - 330),
        ),
        // 270 deg CW: source (x, y) -> target (y, 640 - 1 - x)
        (
            VideoRotation::Degrees270,
            (HEIGHT, WIDTH),
            (14, WIDTH - 1 - 20),
            (330, WIDTH - 1 - 50),
            (330, WIDTH - 1 - 600),
        ),
    ] {
        let config = DecoderConfig::new(
            VideoCodec::H264,
            WIDTH,
            HEIGHT,
            DecoderOutput::Frames { depth: 6 },
        )
        .with_rotation(rotation);
        let decoder = VideoDecoder::new(config).expect("rotated decoder");
        let frames = decoder.frames();
        let mut view = AppLauncher::new()
            .create_embedded_view(view_size.0, view_size.1, 1.0, move || {
                PlayerSurface(
                    frames.clone(),
                    Modifier::empty().fill_max_size(),
                    ContentScale::FillBounds,
                );
            })
            .expect("GPU embedded component");
        let mut pixels = Vec::new();
        draw_settled(&mut view, &mut pixels);
        let mut player = Player {
            decoder,
            view,
            width: view_size.0,
            pixels,
            units: split_access_units(VideoCodec::H264, H264_CLIP),
            fed: 0,
        };
        player.show_next();
        let [red, green, blue] = player.color(clock_at.0, clock_at.1);
        assert!(
            red > green && green > blue && red < 160,
            "{rotation:?}: rotated clock corner at {clock_at:?} is {:?}",
            [red, green, blue]
        );
        assert_points(
            &player,
            &format!("{rotation:?} rotated bars"),
            &[(red_bar_at, [255, 0, 0]), (cyan_bar_at, [0, 255, 255])],
            14,
        );
    }
}

#[test]
fn a_surface_tone_maps_10bit_hdr_pq_and_hlg_streams() {
    for (clip, what, mid_grey_expected, white_expected) in [
        (TONES_2020_PQ, "10-bit BT.2020 PQ", 181_u8, 252_u8),
        (TONES_2020_HLG, "10-bit BT.2020 HLG", 125_u8, 227_u8),
    ] {
        let decoder = VideoDecoder::new(DecoderConfig::new(
            VideoCodec::Hevc,
            320,
            180,
            DecoderOutput::Frames { depth: 6 },
        ))
        .expect("10-bit HEVC decoder");
        let frames = decoder.frames();
        let mut view = AppLauncher::new()
            .create_embedded_view(320, 180, 1.0, move || {
                PlayerSurface(
                    frames.clone(),
                    Modifier::empty().fill_max_size(),
                    ContentScale::FillBounds,
                );
            })
            .expect("GPU embedded component");
        let mut pixels = Vec::new();
        draw_settled(&mut view, &mut pixels);
        let mut player = Player {
            decoder,
            view,
            width: 320,
            pixels,
            units: split_access_units(VideoCodec::Hevc, clip),
            fed: 0,
        };
        player.show_next();
        // Band 0: black, Band 1: tone-mapped 50% encoded grey, Band 2: tone-mapped 75% white
        assert_points(
            &player,
            what,
            &[
                ((32, 90), [0, 0, 0]),
                ((96, 90), [mid_grey_expected; 3]),
                ((160, 90), [white_expected; 3]),
            ],
            14,
        );
        // Band 3: saturated BT.2020 red mapped to sRGB; Band 4: saturated BT.2020 blue
        let red_band = player.color(224, 90);
        assert!(
            red_band[0] > 210 && red_band[1] < 80 && red_band[2] < 80,
            "{what}: red band is {red_band:?}"
        );
        let blue_band = player.color(288, 90);
        assert!(
            blue_band[2] > 210 && blue_band[0] < 80,
            "{what}: blue band is {blue_band:?}"
        );
    }
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "waits for the hardware decoder thread to deliver reordered B-frames"
)]
fn a_surface_presents_b_frame_streams_in_presentation_order() {
    let config = DecoderConfig::new(
        VideoCodec::H264,
        160,
        96,
        DecoderOutput::Frames { depth: 16 },
    )
    .with_order(DecodeOrder::Presentation);
    let mut decoder = VideoDecoder::new(config).expect("B-frame decoder");
    let units = split_access_units(VideoCodec::H264, BFRAMES_H264);
    let shown_order = Rc::new(RefCell::new(Vec::new()));
    let frames = decoder.frames();
    let mut view = AppLauncher::new()
        .create_embedded_view(160, 96, 1.0, {
            let log = Rc::clone(&shown_order);
            move || {
                let log = Rc::clone(&log);
                PlayerSurfaceIntake(
                    frames.clone(),
                    Modifier::empty().fill_max_size(),
                    ContentScale::FillBounds,
                    move |frames, presenter| {
                        while let Some(frame) = frames.next_frame() {
                            log.borrow_mut().push(frame.info().metadata);
                            presenter.present(frame, None).expect("shown b-frame");
                        }
                    },
                );
            }
        })
        .expect("GPU embedded component");
    let mut pixels = Vec::new();
    draw_settled(&mut view, &mut pixels);
    for (data, pts_index) in units.iter().zip(BFRAMES_H264_PTS) {
        decoder
            .submit(AccessUnit {
                data,
                timestamp: STEP * u32::try_from(pts_index).expect("small"),
                metadata: pts_index,
            })
            .expect("b-frame unit");
    }
    decoder.drain().expect("drain b-frames");
    let deadline = Instant::now() + Duration::from_secs(5);
    while shown_order.borrow().len() < BFRAMES_H264_PTS.len() && Instant::now() < deadline {
        view.draw(&mut pixels).expect("frame");
        std::thread::sleep(Duration::from_millis(1));
    }
    let expected: Vec<u64> = (0..BFRAMES_H264_PTS.len() as u64).collect();
    assert_eq!(*shown_order.borrow(), expected);
}
