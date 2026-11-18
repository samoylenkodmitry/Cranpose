// A streamed scene in a window, where no headset shows it. Each test decodes a clip of color bars
// through the platform's decoder and reads the pixels of the window's view. Only Apple's platforms
// run them: no other desktop has a decoder.
#![cfg(any(target_os = "macos", target_os = "ios"))]

use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use cranpose::{
    AccessUnit, AppLauncher, CropRect, DecoderConfig, DecoderOutput, VideoCodec, VideoDecoder,
    embedded_view::EmbeddedView, split_access_units,
};
use cranpose_xr::{
    FieldOfView, FoveatedEncoding, Pose, StreamCounts, StreamStats, StreamedScene, StreamedView,
    StreamedViews,
};

const CLIP: &[u8] = include_bytes!("../../../apps/xr-demo/assets/test_clip.h264");
// The clip is 640 by 360 pixels. Its left half is the left eye's picture, and its right half the
// right eye's.
const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;
const EYE: u32 = WIDTH / 2;
const STEP: Duration = Duration::from_micros(16_667);

// The color bars of the clip from the left: red, green, yellow, blue, magenta and cyan. The
// points lie in the left half and in the right half, in the clip's own pixels, at a row below the
// clock and the diagonal.
const LEFT_BARS: [(u32, [u8; 3]); 3] =
    [(50, [255, 0, 0]), (160, [0, 255, 0]), (270, [255, 255, 0])];
const RIGHT_BARS: [(u32, [u8; 3]); 3] = [
    (50, [0, 0, 255]),
    (160, [255, 0, 255]),
    (270, [0, 255, 255]),
];
const BARS_ROW: u32 = 330;

fn eye(rect: CropRect, foveation: Option<FoveatedEncoding>) -> StreamedView {
    StreamedView {
        pose: Pose::IDENTITY,
        fov: FieldOfView::symmetric(0.4358, 0.4899),
        rect,
        foveation,
    }
}

// The views of a clip frame that hold the left eye in the half that starts at `left_x`, with the
// right eye in the other half.
fn views_with(left_x: u32) -> impl Fn(u64) -> StreamedViews + 'static {
    move |_| {
        let part = |x| CropRect {
            x,
            y: 0,
            width: EYE,
            height: HEIGHT,
        };
        StreamedViews {
            left: eye(part(left_x), None),
            right: eye(part(EYE - left_x), None),
            target_display_time: None,
        }
    }
}

struct Scene {
    decoder: VideoDecoder,
    view: EmbeddedView,
    pixels: Vec<u8>,
    width: u32,
    units: Vec<&'static [u8]>,
    fed: u64,
    stats: StreamStats,
}

impl Scene {
    // A window of `size` pixels that shows a scene with the views of `views`.
    fn new(size: (u32, u32), views: impl Fn(u64) -> StreamedViews + 'static) -> Self {
        let decoder = VideoDecoder::new(DecoderConfig::new(
            VideoCodec::H264,
            WIDTH,
            HEIGHT,
            DecoderOutput::Frames { depth: 6 },
        ))
        .expect("a decoder");
        let frames = decoder.frames();
        let stats = StreamStats::new();
        let scene_stats = stats.clone();
        let views = Rc::new(views);
        let mut view = AppLauncher::new()
            .create_embedded_view(size.0, size.1, 1.0, move || {
                let views = Rc::clone(&views);
                StreamedScene(
                    frames.clone(),
                    move |index| views(index),
                    scene_stats.clone(),
                );
            })
            .expect("GPU embedded component");
        let mut pixels = Vec::new();
        draw_settled(&mut view, &mut pixels);
        Self {
            decoder,
            view,
            pixels,
            width: size.0,
            units: split_access_units(VideoCodec::H264, CLIP),
            fed: 0,
            stats,
        }
    }

    // Gives the decoder the next picture of the clip, whose metadata is its index.
    fn feed(&mut self) {
        let unit = self.units[usize::try_from(self.fed).expect("small") % self.units.len()];
        self.decoder
            .submit(AccessUnit {
                data: unit,
                timestamp: STEP * u32::try_from(self.fed).expect("small"),
                metadata: self.fed,
            })
            .expect("a unit");
        self.fed += 1;
    }

    // Waits until the decoder has output `fed` pictures.
    fn wait_for_output(&self) {
        let frames = self.decoder.frames();
        assert!(
            wait_until(|| frames.stats().decoded >= self.fed),
            "the decoder output no picture"
        );
    }

    // Feeds a picture and draws until it shows.
    fn show_next(&mut self) {
        self.feed();
        self.wait_for_output();
        assert!(self.draw(), "the picture's frame drew nothing");
    }

    // Draws a frame, in which the scene takes the pictures that wait, and tells whether the
    // frame drew anything.
    fn draw(&mut self) -> bool {
        let drawn = self.view.draw(&mut self.pixels).expect("frame");
        draw_settled(&mut self.view, &mut self.pixels);
        drawn
    }

    fn counts(&self) -> StreamCounts {
        self.stats.counts()
    }

    fn color(&self, x: u32, y: u32) -> [u8; 3] {
        let start = ((y * self.width + x) * 4) as usize;
        [
            self.pixels[start],
            self.pixels[start + 1],
            self.pixels[start + 2],
        ]
    }

    // Checks that each bar shows its color at row `row` of the window, `left` columns from the
    // window's left edge.
    fn assert_bars(&self, what: &str, bars: &[(u32, [u8; 3])], left: u32, row: u32) {
        for (x, expected) in bars {
            let color = self.color(left + x, row);
            let close = color
                .iter()
                .zip(expected)
                .all(|(have, want)| have.abs_diff(*want) <= 12);
            assert!(close, "{what}: ({x}, {row}) is {color:?}, not {expected:?}");
        }
    }

    fn assert_blank(&self, what: &str, x: u32, y: u32) {
        let color = self.color(x, y);
        assert!(
            color.iter().all(|channel| *channel < 40),
            "{what}: {color:?}"
        );
    }
}

// Draws as a host does that honors the view's frame schedule: again while the shell asks for a
// frame.
fn draw_settled(view: &mut EmbeddedView, pixels: &mut Vec<u8>) {
    view.draw(pixels).expect("frame");
    let deadline = Instant::now() + Duration::from_secs(30);
    while view.shell().needs_redraw() {
        assert!(Instant::now() < deadline, "the view kept asking for frames");
        std::thread::sleep(Duration::from_millis(2));
        view.draw(pixels).expect("frame");
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

#[test]
fn a_scene_with_no_frames_draws_nothing_and_counts_nothing() {
    let mut scene = Scene::new((EYE, 540), views_with(0));
    assert!(!scene.draw(), "a scene without a frame asked for a draw");
    scene.assert_blank("before a frame", 50, 420);
    assert_eq!(scene.counts(), StreamCounts::default());
}

#[test]
fn a_scene_fits_the_left_eye_to_its_box() {
    // The eye is as wide as the window, and 90 rows of background lie above and below it
    let mut scene = Scene::new((EYE, 540), views_with(0));
    scene.show_next();
    scene.assert_bars("the left eye", &LEFT_BARS, 0, 90 + BARS_ROW);
    // The columns at the eye's edge are yellow, with no blue of the right eye that lies beside
    // them. The last column blends with its neighbor, because the chroma has half the resolution.
    scene.assert_bars(
        "the eye's edge",
        &[(EYE - 2, LEFT_BARS[2].1)],
        0,
        90 + BARS_ROW,
    );
    scene.assert_blank("above the eye", 50, 40);
    scene.assert_blank("below the eye", 50, 500);
    // The same frame in a window that is twice as large fills it, so a point of the window lies
    // at half the distance from the eye's left edge
    let mut scene = Scene::new((EYE * 2, HEIGHT * 2), views_with(0));
    scene.show_next();
    scene.assert_bars(
        "the left eye, doubled",
        &[(100, LEFT_BARS[0].1)],
        0,
        2 * BARS_ROW,
    );
    scene.assert_bars(
        "the left eye, doubled",
        &[(320, LEFT_BARS[1].1)],
        0,
        2 * BARS_ROW,
    );
}

#[test]
fn a_scene_shows_the_part_that_the_views_name() {
    // The views put the left eye in the right half of the frame
    let mut scene = Scene::new((EYE, HEIGHT), views_with(EYE));
    scene.show_next();
    scene.assert_bars("the right half", &RIGHT_BARS, 0, BARS_ROW);
}

#[test]
fn a_scene_counts_the_frames_that_it_shows_and_skips() {
    let asked = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&asked);
    let views = views_with(0);
    let mut scene = Scene::new((EYE, HEIGHT), move |index| {
        log.borrow_mut().push(index);
        views(index)
    });
    // Three frames wait together: the newest shows, and the two before it are skipped
    for _ in 0..3 {
        scene.feed();
    }
    scene.wait_for_output();
    assert!(scene.draw(), "the newest frame drew nothing");
    assert_eq!(
        scene.counts(),
        StreamCounts {
            shown: 1,
            skipped: 2,
            repeated: 0
        }
    );
    // The app's views are asked for every frame, in the order of the stream
    assert_eq!(*asked.borrow(), [0, 1, 2]);
    scene.assert_bars("the newest frame", &LEFT_BARS, 0, BARS_ROW);
    scene.show_next();
    assert_eq!(scene.counts().shown, 2);
    assert_eq!(scene.counts().skipped, 2);
}

#[test]
fn a_scene_keeps_its_picture_when_the_views_of_a_frame_are_refused() {
    // The rectangle of every frame after the first lies outside the picture
    let good = views_with(0);
    let mut scene = Scene::new((EYE, HEIGHT), move |index| {
        let mut views = good(index);
        if index > 0 {
            views.left.rect.x = WIDTH;
        }
        views
    });
    scene.show_next();
    scene.feed();
    scene.wait_for_output();
    assert!(!scene.draw(), "a refused frame asked for a draw");
    scene.assert_bars("the picture before", &LEFT_BARS, 0, BARS_ROW);
    assert_eq!(
        scene.counts(),
        StreamCounts {
            shown: 1,
            skipped: 1,
            repeated: 0
        }
    );
}

#[test]
fn a_scene_shows_a_foveated_picture_as_the_host_encoded_it() {
    // The flat scene has no unwarp, so it shows the rectangle as it is and drops no frame
    let plain = views_with(0);
    let mut scene = Scene::new((EYE, HEIGHT), move |index| {
        let mut views = plain(index);
        views.left.foveation = Some(FoveatedEncoding {
            full_size: (EYE * 2, HEIGHT * 2),
            center_size: [0.5, 0.5],
            center_shift: [0.0, 0.0],
            edge_ratio: [2.0, 2.0],
            eye_size_ratio: [1.0, 1.0],
            mirrored: false,
        });
        views
    });
    scene.show_next();
    scene.assert_bars("the encoded eye", &LEFT_BARS, 0, BARS_ROW);
    assert_eq!(scene.counts().shown, 1);
    assert_eq!(scene.counts().skipped, 0);
}
