// The Music tab's sound, rendered without a device: the player hands its
// renderer to a sink that keeps it, and each test pulls the frames itself.

use std::{
    cell::RefCell,
    f32::consts::TAU,
    rc::Rc,
    sync::{Mutex, MutexGuard, PoisonError},
};

use cranpose_audio::{
    backend::{AudioSink, Renderer},
    RenderStatus,
};
use cranpose_core::CompositionLocalProvider;
use cranpose_services::{advance_lifecycle, LifecycleState};
use cranpose_testing::robot::{RobotTestRule, TestRenderer};
use desktop_app::app::{
    combined_app_with_initial_tab,
    music::{
        clair_de_lune, local_music_output, KeyRange, MusicOutput, MusicPlayer, Note, Notes, Timbre,
        Voicing,
    },
    DemoTab, TEST_ACTIVE_TAB_STATE,
};

const SAMPLE_RATE: f32 = 48_000.0;
const CHANNELS: usize = 2;
// Under this level a frame counts as silent: 100 dB below full scale.
const SILENT: f32 = 1.0e-5;

// The whole part of a count of frames.
fn count(frames: f32) -> usize {
    usize::try_from(cranpose_ui_graphics::whole::to_i32(frames)).unwrap_or(0)
}

struct KeptSink;

impl AudioSink for KeptSink {}

// An output that keeps the renderer of the player that opens it.
struct KeptOutput {
    output: MusicOutput,
    renderer: Rc<RefCell<Option<Box<dyn Renderer>>>>,
}

impl KeptOutput {
    fn new() -> Self {
        let renderer = Rc::new(RefCell::new(None));
        let kept = Rc::clone(&renderer);
        let output = MusicOutput::new(move |renderer| {
            *kept.borrow_mut() = Some(renderer);
            Ok(Box::new(KeptSink))
        });
        Self { output, renderer }
    }

    // The left channel of the next `seconds` of sound.
    fn render(&self, seconds: f32) -> Vec<f32> {
        let frames = count(seconds * SAMPLE_RATE);
        let mut out = vec![0.0; frames * CHANNELS];
        let mut renderer = self.renderer.borrow_mut();
        let renderer = renderer
            .as_mut()
            .expect("the player opens its device at the first sound");
        renderer.set_device_format(SAMPLE_RATE, CHANNELS);
        // A device asks for a few hundred frames at a time.
        for block in out.chunks_mut(480 * CHANNELS) {
            let _: RenderStatus = renderer.render(block);
        }
        let (frames, _) = out.as_chunks::<CHANNELS>();
        frames.iter().map(|frame| frame[0]).collect()
    }
}

struct Stage {
    player: MusicPlayer,
    output: KeptOutput,
}

impl Stage {
    fn new() -> Self {
        let output = KeptOutput::new();
        Self {
            player: MusicPlayer::new(output.output.clone()),
            output,
        }
    }

    fn render(&self, seconds: f32) -> Vec<f32> {
        self.output.render(seconds)
    }
}

// The host's lifecycle is one for the process. The tests of the tab run one at
// a time, so the test that takes the app off the screen stops no other
// test's piece.
fn alone() -> MutexGuard<'static, ()> {
    static HOST: Mutex<()> = Mutex::new(());
    HOST.lock().unwrap_or_else(PoisonError::into_inner)
}

// The demo on its Music tab, with the sound kept by `output`.
fn music_tab(output: &KeptOutput) -> RobotTestRule<TestRenderer> {
    let output = output.output.clone();
    let mut robot = RobotTestRule::new(1200, 800, TestRenderer::default(), move || {
        CompositionLocalProvider([local_music_output().provides(output.clone())], || {
            combined_app_with_initial_tab(Some(DemoTab::Music));
        });
    });
    robot.wait_for_idle();
    robot
}

fn shows(robot: &mut RobotTestRule<TestRenderer>, text: &str) -> bool {
    robot
        .get_all_text()
        .iter()
        .any(|shown| shown.contains(text))
}

// Lets `seconds` pass for the sound and for the picture, a frame at a time.
fn play_for(
    robot: &mut RobotTestRule<TestRenderer>,
    output: &KeptOutput,
    seconds: f32,
) -> Vec<f32> {
    let mut sound = Vec::new();
    for _ in 0..count(seconds * 60.0) {
        sound.extend(output.render(1.0 / 60.0));
        robot.advance_time(1_000_000_000 / 60);
    }
    sound
}

fn pure() -> Voicing {
    Voicing {
        timbre: Timbre::Pure,
        ..Voicing::default()
    }
}

// How much of `samples` is a sine of `hertz`.
fn level_at(samples: &[f32], hertz: f32) -> f32 {
    let turn = TAU * hertz / SAMPLE_RATE;
    let (mut sin, mut cos) = (0.0, 0.0);
    for (index, sample) in samples.iter().enumerate() {
        let angle = turn * index as f32;
        sin += sample * angle.sin();
        cos += sample * angle.cos();
    }
    (sin * sin + cos * cos).sqrt() / samples.len() as f32
}

fn first_sound(samples: &[f32]) -> Option<usize> {
    samples.iter().position(|sample| sample.abs() > SILENT)
}

fn one_note(start: f32, key: u8) -> Notes {
    Notes::new(vec![Note {
        start,
        held: 0.25,
        ring: 0.25,
        key,
        velocity: 100,
    }])
}

#[test]
fn clair_de_lune_reads_as_two_hands() {
    let song = clair_de_lune();
    let names: Vec<_> = song.parts.iter().map(|part| part.name.as_str()).collect();
    assert_eq!(names, ["Piano right", "Piano left"]);
    assert_eq!(song.parts[0].notes.as_slice().len(), 712);
    assert_eq!(song.parts[1].notes.as_slice().len(), 779);
    assert!(
        (240.0..280.0).contains(&song.seconds()),
        "the piece lasts about four minutes, not {} s",
        song.seconds()
    );
    for part in &song.parts {
        let notes = part.notes.as_slice();
        assert!(notes.windows(2).all(|pair| pair[0].start <= pair[1].start));
        assert!(notes.iter().all(|note| note.ring >= note.held));
    }
    // The pedal holds the left hand's notes past the lift of their keys.
    let pedalled = song.parts[1]
        .notes
        .as_slice()
        .iter()
        .filter(|note| note.ring > note.held)
        .count();
    assert!(pedalled > 300, "{pedalled} notes ring under the pedal");
}

#[test]
fn a_key_by_hand_sounds_its_pitch_until_it_lifts() {
    let stage = Stage::new();
    stage.player.key_down(69, 100, pure());
    let held = stage.render(0.5);
    let a4 = level_at(&held, 440.0);
    assert!(a4 > 0.01, "the A above middle C sounds at 440 Hz: {a4}");
    assert!(a4 > 20.0 * level_at(&held, 466.16));
    assert!(a4 > 20.0 * level_at(&held, 415.3));

    stage.player.key_up(69);
    stage.render(4.0);
    let after = stage.render(0.1);
    assert_eq!(first_sound(&after), None, "the room is silent again");
}

#[test]
fn transpose_moves_a_key_by_semitones() {
    let stage = Stage::new();
    stage.player.key_down(
        69,
        100,
        Voicing {
            transpose: 12,
            ..pure()
        },
    );
    let held = stage.render(0.5);
    assert!(level_at(&held, 880.0) > 20.0 * level_at(&held, 440.0));
}

#[test]
fn a_part_starts_its_note_on_the_frame_of_its_start() {
    let stage = Stage::new();
    stage.player.set_length(10.0);
    let slot = stage.player.add_part(&one_note(0.5, 60)).unwrap();
    stage.player.voice_part(slot, pure());
    stage.player.play();
    let sound = stage.render(1.0);
    assert_eq!(first_sound(&sound), Some(24_000));
}

#[test]
fn tempo_scales_the_time_to_a_note() {
    let stage = Stage::new();
    stage.player.set_length(10.0);
    stage.player.set_tempo(2.0);
    let slot = stage.player.add_part(&one_note(0.5, 60)).unwrap();
    stage.player.voice_part(slot, pure());
    stage.player.play();
    let sound = stage.render(1.0);
    assert_eq!(first_sound(&sound), Some(12_000));
    assert!((stage.player.position() - 2.0).abs() < 1.0e-6);
}

#[test]
fn a_paused_clock_stands_still_and_a_seek_moves_it() {
    let stage = Stage::new();
    stage.player.set_length(10.0);
    let slot = stage.player.add_part(&one_note(3.0, 60)).unwrap();
    stage.player.voice_part(slot, pure());
    stage.player.play();
    stage.render(0.5);
    stage.player.pause();
    stage.render(0.5);
    assert!((stage.player.position() - 0.5).abs() < 1.0e-6);

    stage.player.seek(2.75);
    stage.player.play();
    let sound = stage.render(0.5);
    assert_eq!(first_sound(&sound), Some(12_000));
}

#[test]
fn the_clock_stops_at_the_length_of_the_song() {
    let stage = Stage::new();
    stage.player.set_length(1.0);
    stage.player.play();
    stage.render(1.5);
    assert!(!stage.player.is_playing());
    assert!(stage.player.position() >= 1.0);
    assert!(stage.player.position() < 1.02);
}

#[test]
fn a_removed_part_plays_no_more_notes() {
    let stage = Stage::new();
    stage.player.set_length(10.0);
    let slot = stage.player.add_part(&one_note(0.5, 60)).unwrap();
    stage.player.voice_part(slot, pure());
    stage.player.play();
    stage.render(0.25);
    stage.player.remove_part(slot);
    let sound = stage.render(1.0);
    assert_eq!(first_sound(&sound), None);
}

#[test]
fn a_zero_volume_part_is_silent() {
    let stage = Stage::new();
    stage.player.set_length(10.0);
    let slot = stage.player.add_part(&one_note(0.1, 60)).unwrap();
    stage.player.voice_part(
        slot,
        Voicing {
            volume: 0.0,
            ..pure()
        },
    );
    stage.player.play();
    assert_eq!(first_sound(&stage.render(1.0)), None);
}

#[test]
fn the_whole_piece_stays_under_full_scale() {
    let song = clair_de_lune();
    let stage = Stage::new();
    stage.player.set_length(song.seconds());
    stage.player.set_tempo(4.0);
    for part in &song.parts {
        let slot = stage.player.add_part(&part.notes).unwrap();
        stage.player.voice_part(slot, Voicing::default());
    }
    stage.player.play();
    let sound = stage.render(song.seconds() / 4.0 + 1.0);
    let peak = sound
        .iter()
        .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
    let power = sound.iter().map(|sample| sample * sample).sum::<f32>() / sound.len() as f32;
    eprintln!("clair de lune: peak {peak:.3}, rms {:.3}", power.sqrt());
    assert!(peak <= 1.0, "peak {peak}");
    assert!(power.sqrt() > 0.01, "the piece is audible");
    assert!(!stage.player.is_playing(), "the clock stopped at the end");
}

#[test]
fn the_keys_down_follow_the_notes_and_their_transposition() {
    let stage = Stage::new();
    let slot = stage.player.add_part(&one_note(1.0, 60)).unwrap();
    stage.player.voice_part(
        slot,
        Voicing {
            transpose: 2,
            ..pure()
        },
    );
    assert!(stage.player.keys_down_at(0.9).is_empty());
    let down = stage.player.keys_down_at(1.1);
    assert!(down.contains(62) && down.len() == 1);
    assert!(stage.player.keys_down_at(1.3).is_empty());
}

#[test]
fn play_moves_the_clock_of_the_tab_and_pause_holds_it() {
    let _alone = alone();
    let output = KeptOutput::new();
    let mut robot = music_tab(&output);
    assert!(shows(&mut robot, "Clair de lune, Claude Debussy"));
    assert!(shows(&mut robot, "0:00 / 4:"));
    assert!(
        output.renderer.borrow().is_none(),
        "no device before a sound"
    );

    assert!(robot.find_by_text("Play").click());
    let sound = play_for(&mut robot, &output, 2.5);
    assert!(shows(&mut robot, "Pause"));
    assert!(shows(&mut robot, "0:02 / 4:"));
    assert!(first_sound(&sound).is_some(), "the piece sounds");

    assert!(robot.find_by_text("Pause").click());
    play_for(&mut robot, &output, 2.0);
    robot.wait_for_idle();
    assert!(shows(&mut robot, "Play"));
    assert!(shows(&mut robot, "0:02 / 4:"));
    // The dampers fell at the pause: the notes and their room have faded.
    output.render(3.0);
    assert_eq!(first_sound(&output.render(0.5)), None);
}

#[test]
fn a_hand_turned_off_is_silent_until_it_is_turned_on() {
    let _alone = alone();
    let output = KeptOutput::new();
    let mut robot = music_tab(&output);
    assert!(robot.find_by_text("Piano right").click());
    assert!(robot.find_by_text("Piano left").click());
    robot.wait_for_idle();
    assert!(robot.find_by_text("Play").click());
    let silence = play_for(&mut robot, &output, 3.0);
    assert_eq!(first_sound(&silence), None);

    assert!(robot.find_by_text("Piano right").click());
    let sound = play_for(&mut robot, &output, 3.0);
    assert!(first_sound(&sound).is_some());
}

#[test]
fn a_press_on_the_keyboard_sounds_the_key_under_it() {
    let _alone = alone();
    let output = KeptOutput::new();
    let mut robot = music_tab(&output);
    assert!(robot.find_by_text("Pure tone").click());
    robot.wait_for_idle();
    let keyboard = robot
        .find_by_tag("MusicKeyboard")
        .bounds()
        .expect("the tab shows a keyboard");
    // Low on the keyboard, under the black keys, at its middle.
    let x = keyboard.x + keyboard.width * 0.5;
    let y = keyboard.y + keyboard.height * 0.9;
    let key = KeyRange::PIANO
        .key_at(x - keyboard.x, 0.9, keyboard.width)
        .expect("the middle of the keyboard holds a key");
    let hertz = 440.0 * 2f32.powf((f32::from(key) - 69.0) / 12.0);

    robot.mouse_move(x, y);
    robot.mouse_down();
    let held = play_for(&mut robot, &output, 0.5);
    let level = level_at(&held, hertz);
    assert!(level > 0.01, "key {key} sounds at {hertz} Hz: {level}");
    assert!(level > 20.0 * level_at(&held, hertz * 1.5));

    robot.mouse_up();
    play_for(&mut robot, &output, 4.0);
    assert_eq!(first_sound(&play_for(&mut robot, &output, 0.2)), None);
}

#[test]
fn leaving_the_tab_stops_the_piece() {
    let _alone = alone();
    let output = KeptOutput::new();
    let mut robot = music_tab(&output);
    assert!(robot.find_by_text("Play").click());
    play_for(&mut robot, &output, 1.5);

    TEST_ACTIVE_TAB_STATE.with(|state| {
        state
            .borrow()
            .expect("the demo keeps its tab")
            .set(DemoTab::Counter);
    });
    robot.wait_for_idle();
    // The notes that sounded fade with their room; no note follows them.
    output.render(8.0);
    assert_eq!(first_sound(&output.render(1.0)), None);
}

#[test]
fn the_piece_stops_when_the_app_leaves_the_screen() {
    let _alone = alone();
    let output = KeptOutput::new();
    let mut robot = music_tab(&output);
    advance_lifecycle(LifecycleState::Resumed);
    assert!(robot.find_by_text("Play").click());
    play_for(&mut robot, &output, 1.0);
    assert!(shows(&mut robot, "Pause"));

    advance_lifecycle(LifecycleState::Stopped);
    play_for(&mut robot, &output, 0.1);
    // A piece that still played would move its clock each frame, and the
    // wait for an idle screen would not end.
    robot.wait_for_idle();
    advance_lifecycle(LifecycleState::Resumed);
    robot.wait_for_idle();
    assert!(shows(&mut robot, "Play"), "the piece waits for a press");
    output.render(8.0);
    assert_eq!(first_sound(&output.render(1.0)), None);
}
