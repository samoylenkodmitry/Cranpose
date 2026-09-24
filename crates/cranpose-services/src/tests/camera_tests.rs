use std::sync::PoisonError;

use super::*;

struct FakeCamera {
    started: AtomicU64,
    stopped: AtomicU64,
    stills: AtomicU64,
    fails: bool,
}

impl FakeCamera {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            started: AtomicU64::new(0),
            stopped: AtomicU64::new(0),
            stills: AtomicU64::new(0),
            fails: false,
        })
    }

    fn failing() -> Arc<Self> {
        Arc::new(Self {
            started: AtomicU64::new(0),
            stopped: AtomicU64::new(0),
            stills: AtomicU64::new(0),
            fails: true,
        })
    }
}

impl Camera for FakeCamera {
    fn start(&self) -> Result<(), CameraError> {
        self.started.fetch_add(1, Ordering::Relaxed);
        if self.fails {
            return Err(CameraError::PermissionDenied);
        }
        publish_camera_state(CameraState::Running {
            device: "fake".to_string(),
        });
        Ok(())
    }

    fn stop(&self) {
        self.stopped.fetch_add(1, Ordering::Relaxed);
    }

    fn request_still(&self) -> Result<(), CameraError> {
        self.stills.fetch_add(1, Ordering::Relaxed);
        publish_camera_still(Ok(CameraStill {
            jpeg: vec![0xff, 0xd8],
        }));
        Ok(())
    }
}

fn rgba_frame(sequence: u64) -> CameraFrame {
    CameraFrame::new(2, 2, FrameFormat::Rgba8, 90, sequence, vec![7; 16])
        .expect("a well-formed frame")
}

#[test]
fn a_frame_size_is_the_one_its_format_implies() {
    assert_eq!(FrameFormat::Rgba8.byte_len(4, 2), Some(32));
    assert_eq!(FrameFormat::Nv12.byte_len(4, 2), Some(12));
    assert_eq!(
        FrameFormat::Nv12.byte_len(3, 2),
        None,
        "NV12 has no half column for an odd width"
    );
    assert_eq!(FrameFormat::Nv12.byte_len(4, 3), None);
}

#[test]
fn a_frame_that_does_not_match_its_size_is_refused() {
    assert!(CameraFrame::new(2, 2, FrameFormat::Rgba8, 0, 0, vec![0; 15]).is_none());
    assert!(CameraFrame::new(2, 2, FrameFormat::Nv12, 0, 0, vec![0; 5]).is_none());
    assert!(CameraFrame::new(2, 2, FrameFormat::Rgba8, 0, 0, vec![0; 16]).is_some());
}

#[test]
fn a_rotation_is_kept_inside_one_turn() {
    let frame = CameraFrame::new(2, 2, FrameFormat::Rgba8, 450, 0, vec![0; 16])
        .expect("a well-formed frame");
    assert_eq!(frame.rotation_degrees, 90);
}

#[test]
fn an_rgba_frame_is_handed_over_unchanged() {
    let frame = rgba_frame(0);
    assert_eq!(frame.to_rgba8(), frame.bytes);
}

#[test]
fn nv12_black_white_and_primaries_convert_to_the_expected_colours() {
    fn convert(luma: u8, blue: u8, red: u8) -> [u8; 4] {
        let bytes = vec![luma, luma, luma, luma, blue, red];
        let frame =
            CameraFrame::new(2, 2, FrameFormat::Nv12, 0, 0, bytes).expect("a well-formed frame");
        let rgba = frame.to_rgba8();
        [rgba[0], rgba[1], rgba[2], rgba[3]]
    }

    assert_eq!(convert(0, 128, 128), [0, 0, 0, 255], "black");
    assert_eq!(convert(255, 128, 128), [255, 255, 255, 255], "white");

    let red = convert(76, 84, 255);
    assert!(
        red[0] > 240 && red[1] < 20 && red[2] < 20,
        "red, got {red:?}"
    );
    let blue = convert(29, 255, 107);
    assert!(
        blue[2] > 240 && blue[0] < 20 && blue[1] < 20,
        "blue, got {blue:?}"
    );
    assert!(
        convert(128, 128, 128).iter().all(|value| *value > 0),
        "a grey frame must not clamp to black"
    );
}

#[test]
fn a_short_nv12_frame_converts_to_black_rather_than_reading_past_its_end() {
    let rgba = nv12_to_rgba8(4, 4, &[0u8; 3]);
    assert_eq!(rgba.len(), 4 * 4 * 4);
    assert!(rgba.iter().all(|value| *value == 0));
}

#[test]
fn a_platform_without_a_camera_says_so_rather_than_pretending() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_camera();
    assert!(!camera_supported());
    assert_eq!(start_camera(), Err(CameraError::Unsupported));
    assert_eq!(
        camera_state(),
        CameraState::Failed(CameraError::Unsupported)
    );
    assert_eq!(request_camera_still(), Err(CameraError::Unsupported));
    clear_platform_camera();
}

#[test]
fn the_session_reports_starting_before_it_reports_running() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_camera();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&seen);
    let observer = observe_camera_state(move |state| {
        recorder
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(state);
    });
    set_platform_camera(FakeCamera::new());
    start_camera().expect("the session starts");

    assert_eq!(
        *seen.lock().unwrap_or_else(PoisonError::into_inner),
        vec![
            CameraState::Idle,
            CameraState::Starting,
            CameraState::Running {
                device: "fake".to_string()
            }
        ]
    );
    assert!(camera_state().is_running());
    assert!(camera_state().is_active());
    drop(observer);
    clear_platform_camera();
}

#[test]
fn a_session_that_cannot_open_reports_why() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_camera();
    set_platform_camera(FakeCamera::failing());
    assert_eq!(start_camera(), Err(CameraError::PermissionDenied));
    assert_eq!(
        camera_state().failure(),
        Some(&CameraError::PermissionDenied)
    );
    assert!(!camera_state().is_active());
    clear_platform_camera();
}

#[test]
fn stopping_releases_the_device_and_says_so() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_camera();
    let backend = FakeCamera::new();
    set_platform_camera(backend.clone());
    start_camera().expect("the session starts");
    stop_camera();
    assert_eq!(backend.stopped.load(Ordering::Relaxed), 1);
    assert_eq!(camera_state(), CameraState::Stopped);
    clear_platform_camera();
}

#[test]
fn the_stored_frame_is_always_the_newest_one() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_camera();
    assert_eq!(latest_camera_frame(), None);
    publish_camera_frame(rgba_frame(1));
    publish_camera_frame(rgba_frame(2));
    publish_camera_frame(rgba_frame(3));
    assert_eq!(latest_camera_frame().map(|frame| frame.sequence), Some(3));
    clear_platform_camera();
}

#[test]
fn frame_observers_see_frames_and_stop_when_dropped() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_camera();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&seen);
    let observer = observe_camera_frames(move |frame| {
        recorder
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(frame.sequence);
    });
    publish_camera_frame(rgba_frame(1));
    publish_camera_frame(rgba_frame(2));
    drop(observer);
    publish_camera_frame(rgba_frame(3));
    assert_eq!(
        *seen.lock().unwrap_or_else(PoisonError::into_inner),
        vec![1, 2]
    );
    clear_platform_camera();
}

#[test]
fn frames_the_platform_could_not_deliver_are_counted_rather_than_queued() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_camera();
    assert_eq!(dropped_camera_frames(), 0);
    record_dropped_camera_frame();
    record_dropped_camera_frame();
    assert_eq!(dropped_camera_frames(), 2);
    publish_camera_state(CameraState::Starting);
    assert_eq!(dropped_camera_frames(), 0);
    assert_eq!(latest_camera_frame(), None);
    clear_platform_camera();
}

#[test]
fn a_still_is_asked_for_and_arrives_separately() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_camera();
    let backend = FakeCamera::new();
    set_platform_camera(backend.clone());

    assert_eq!(
        request_camera_still(),
        Err(CameraError::NotRunning),
        "nothing is running, so there is nothing to photograph"
    );

    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&seen);
    let observer = observe_camera_stills(move |still| {
        recorder
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(still);
    });
    start_camera().expect("the session starts");
    request_camera_still().expect("a still is asked for");
    assert_eq!(backend.stills.load(Ordering::Relaxed), 1);
    assert_eq!(
        *seen.lock().unwrap_or_else(PoisonError::into_inner),
        vec![Ok(CameraStill {
            jpeg: vec![0xff, 0xd8]
        })]
    );
    drop(observer);
    clear_platform_camera();
}

#[test]
fn a_backend_that_lists_no_lens_and_no_flash_says_so() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_camera();
    set_platform_camera(FakeCamera::new());
    let backend = camera().expect("registered");
    assert!(backend.lenses().is_empty());
    assert_eq!(backend.lens(), None);
    assert!(!backend.use_lens("0"));
    assert!(!backend.has_flash());
    assert!(!backend.set_flash(FlashMode::On));
    assert!(!backend.set_torch(true));
    clear_platform_camera();
}

#[test]
fn a_backend_that_lists_two_lenses_hands_them_over_in_order() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_camera();
    struct TwoLenses;
    impl Camera for TwoLenses {
        fn start(&self) -> Result<(), CameraError> {
            Ok(())
        }
        fn stop(&self) {}
        fn lenses(&self) -> Vec<CameraLens> {
            vec![
                CameraLens {
                    id: "u".into(),
                    name: "Ultra wide".into(),
                    facing: LensFacing::Back,
                },
                CameraLens {
                    id: "w".into(),
                    name: "Wide".into(),
                    facing: LensFacing::Back,
                },
            ]
        }
        fn lens(&self) -> Option<String> {
            Some("w".into())
        }
        fn use_lens(&self, id: &str) -> bool {
            id == "u" || id == "w"
        }
    }
    set_platform_camera(Arc::new(TwoLenses));
    let backend = camera().expect("registered");
    let lenses = backend.lenses();
    assert_eq!(lenses.len(), 2);
    assert_eq!(lenses[0].name, "Ultra wide");
    assert_eq!(backend.lens().as_deref(), Some("w"));
    assert!(backend.use_lens("u"));
    assert!(!backend.use_lens("tele"));
    clear_platform_camera();
}

fn two_pixel_frame(rotation: u16) -> CameraFrame {
    let mut bytes = vec![10u8, 10, 10, 255];
    bytes.extend_from_slice(&[20, 20, 20, 255]);
    CameraFrame::new(2, 1, FrameFormat::Rgba8, rotation, 0, bytes).expect("a well-formed frame")
}

fn pixel_values(image: &UprightRgba) -> Vec<u8> {
    image.rgba.iter().step_by(4).copied().collect()
}

#[test]
fn a_frame_turns_upright_by_its_rotation() {
    let unturned = two_pixel_frame(0).upright_rgba8();
    assert_eq!((unturned.width, unturned.height), (2, 1));
    assert_eq!(pixel_values(&unturned), vec![10, 20]);

    let quarter = two_pixel_frame(90).upright_rgba8();
    assert_eq!((quarter.width, quarter.height), (1, 2));
    assert_eq!(pixel_values(&quarter), vec![10, 20]);

    let half = two_pixel_frame(180).upright_rgba8();
    assert_eq!((half.width, half.height), (2, 1));
    assert_eq!(pixel_values(&half), vec![20, 10]);

    let three_quarters = two_pixel_frame(270).upright_rgba8();
    assert_eq!((three_quarters.width, three_quarters.height), (1, 2));
    assert_eq!(pixel_values(&three_quarters), vec![20, 10]);
}

#[test]
fn an_nv12_frame_turns_and_converts_in_one_pass() {
    let bytes = vec![0, 255, 0, 0, 128, 128];
    let frame =
        CameraFrame::new(2, 2, FrameFormat::Nv12, 90, 0, bytes).expect("a well-formed frame");
    let upright = frame.upright_rgba8();
    assert_eq!((upright.width, upright.height), (2, 2));
    let values = pixel_values(&upright);
    assert_eq!(values[0], 0, "top-left stays dark");
    assert_eq!(
        values[3], 255,
        "the bright top-right pixel lands bottom-right"
    );
}

#[test]
fn an_rgb8_frame_turns_upright_with_a_full_alpha() {
    let frame = CameraFrame::new(
        2,
        1,
        FrameFormat::Rgb8,
        180,
        0,
        vec![10, 10, 10, 20, 20, 20],
    )
    .expect("a well-formed frame");
    let upright = frame.upright_rgba8();
    assert_eq!(pixel_values(&upright), vec![20, 10]);
    assert!(upright.rgba.iter().skip(3).step_by(4).all(|a| *a == 255));
}

#[test]
fn a_turn_that_is_not_a_quarter_is_left_alone() {
    let frame =
        CameraFrame::new(2, 1, FrameFormat::Rgba8, 45, 0, vec![7; 8]).expect("a well-formed frame");
    let upright = frame.upright_rgba8();
    assert_eq!((upright.width, upright.height), (2, 1));
    assert_eq!(upright.rgba, frame.to_rgba8());
}

#[test]
fn the_lens_list_is_published_and_observed() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_camera();
    assert_eq!(camera_lenses(), CameraLenses::default());

    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&seen);
    let observer = observe_camera_lenses(move |lenses| {
        recorder
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(lenses);
    });

    let published = CameraLenses {
        lenses: vec![CameraLens {
            id: "0".into(),
            name: "Back".into(),
            facing: LensFacing::Back,
        }],
        active: Some("0".into()),
    };
    publish_camera_lenses(published.clone());
    publish_camera_lenses(published.clone());
    assert_eq!(camera_lenses(), published);
    assert_eq!(
        *seen.lock().unwrap_or_else(PoisonError::into_inner),
        vec![CameraLenses::default(), published],
        "the current list arrives at once, and a repeat is not re-delivered"
    );

    drop(observer);
    publish_camera_lenses(CameraLenses::default());
    assert_eq!(
        seen.lock().unwrap_or_else(PoisonError::into_inner).len(),
        2,
        "a dropped observer hears nothing more"
    );
    clear_platform_camera();
    assert_eq!(camera_lenses(), CameraLenses::default());
}
