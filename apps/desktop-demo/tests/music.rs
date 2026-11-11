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
use cranpose_ui::{Color, Key, KeyEvent};
use desktop_app::app::{
    combined_app_with_initial_tab,
    music::{
        clair_de_lune, local_music_output, KeyRange, MusicOutput, MusicPlayer, Note, NotePlace,
        Notes, PartSlot, Song, Timbre, Voicing,
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
        Self::rendering_as_it_opens(0)
    }

    // An output that renders `frames` inside the call that opens it, before
    // the player has its sink. The Web Audio backend does that.
    fn rendering_as_it_opens(frames: usize) -> Self {
        let renderer = Rc::new(RefCell::new(None));
        let kept = Rc::clone(&renderer);
        let output = MusicOutput::new(move |mut renderer| {
            if frames > 0 {
                renderer.set_device_format(SAMPLE_RATE, CHANNELS);
                let _: RenderStatus = renderer.render(&mut vec![0.0; frames * CHANNELS]);
            }
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

    // Holds a part of `notes` with `voicing` and every one of its notes,
    // as the note composables of a roll that shows all of them do.
    fn hold(&self, notes: &Notes, voicing: Voicing) -> Held {
        let part = self
            .player
            .hold_part(notes, Color::WHITE)
            .expect("a new player has a free slot");
        self.player.voice_part(part, voicing);
        let places = (0u32..)
            .zip(notes.as_slice())
            .map(|(index, note)| {
                self.player
                    .hold_note(part, index, *note)
                    .expect("a few notes find a place")
            })
            .collect();
        Held { part, places }
    }
}

// A part a test holds, and the places of its notes in their order.
struct Held {
    part: PartSlot,
    places: Vec<NotePlace>,
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

// Opens the sound settings of the panel.
fn open_sound(robot: &mut RobotTestRule<TestRenderer>) {
    assert!(
        robot.find_by_text("Sound").click(),
        "the panel has a Sound chip"
    );
    robot.wait_for_idle();
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
fn a_held_note_starts_on_the_frame_of_its_start() {
    let stage = Stage::new();
    stage.player.set_length(10.0);
    stage.hold(&one_note(0.5, 60), pure());
    stage.player.play();
    let sound = stage.render(1.0);
    assert_eq!(first_sound(&sound), Some(24_000));
}

#[test]
fn a_note_nothing_holds_does_not_sound() {
    let stage = Stage::new();
    stage.player.set_length(10.0);
    let notes = one_note(0.5, 60);
    let part = stage
        .player
        .hold_part(&notes, Color::WHITE)
        .expect("a new player has a free slot");
    stage.player.voice_part(part, pure());
    stage.player.play();
    assert_eq!(first_sound(&stage.render(1.0)), None);
}

#[test]
fn a_note_held_twice_sounds_until_both_holders_let_go() {
    let stage = Stage::new();
    stage.player.set_length(10.0);
    let notes = long_note();
    let held = stage.hold(&notes, pure());
    let again = stage
        .player
        .hold_note(held.part, 0, notes.as_slice()[0])
        .expect("the note has a place already");
    assert_eq!(again, held.places[0], "one note takes one place");
    stage.player.play();
    stage.render(1.5);

    stage.player.release_note(again);
    assert!(
        settled_level(&stage.render(0.3)) > 0.01,
        "one holder is left"
    );
    stage.player.release_note(held.places[0]);
    stage.render(4.0);
    assert_eq!(first_sound(&stage.render(0.2)), None, "no holder is left");
}

#[test]
fn tempo_scales_the_time_to_a_note() {
    let stage = Stage::new();
    stage.player.set_length(10.0);
    stage.player.set_tempo(2.0);
    stage.hold(&one_note(0.5, 60), pure());
    stage.player.play();
    let sound = stage.render(1.0);
    assert_eq!(first_sound(&sound), Some(12_000));
    assert!((stage.player.position() - 2.0).abs() < 1.0e-6);
}

#[test]
fn a_paused_clock_stands_still_and_a_seek_moves_it() {
    let stage = Stage::new();
    stage.player.set_length(10.0);
    stage.hold(&one_note(3.0, 60), pure());
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
fn a_seek_before_the_first_sound_holds_on_a_device_that_renders_as_it_opens() {
    let output = KeptOutput::rendering_as_it_opens(1024);
    let stage = Stage {
        player: MusicPlayer::new(output.output.clone()),
        output,
    };
    stage.player.set_length(10.0);
    stage.hold(&one_note(3.0, 60), pure());
    stage.player.seek(2.75);
    stage.player.play();
    let sound = stage.render(0.5);
    assert_eq!(first_sound(&sound), Some(12_000));
    assert!((stage.player.position() - 3.25).abs() < 1.0e-6);
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
fn a_part_let_go_of_falls_silent() {
    let stage = Stage::new();
    stage.player.set_length(10.0);
    let held = stage.hold(&long_note(), pure());
    stage.player.play();
    assert!(settled_level(&stage.render(1.5)) > 0.01);
    stage.player.release_part(held.part);
    stage.render(4.0);
    assert_eq!(first_sound(&stage.render(0.2)), None);
}

#[test]
fn a_zero_volume_part_is_silent() {
    let stage = Stage::new();
    stage.player.set_length(10.0);
    stage.hold(
        &one_note(0.1, 60),
        Voicing {
            volume: 0.0,
            ..pure()
        },
    );
    stage.player.play();
    assert_eq!(first_sound(&stage.render(1.0)), None);
}

// Plays `song` at four times its tempo the way a roll does: each note is
// held from a few seconds before the clock reaches it until it has rung out.
fn perform(stage: &Stage, song: &Song) -> Vec<f32> {
    const AHEAD_SECONDS: f64 = 6.0;
    let parts: Vec<_> = song
        .parts
        .iter()
        .map(|part| {
            let slot = stage
                .player
                .hold_part(&part.notes, Color::WHITE)
                .expect("the player has a slot for each hand");
            stage.player.voice_part(slot, Voicing::default());
            (slot, &part.notes)
        })
        .collect();
    let mut next = vec![0usize; parts.len()];
    let mut held: Vec<(f32, NotePlace)> = Vec::new();
    let mut sound = Vec::new();
    stage.player.set_tempo(4.0);
    stage.player.play();
    while stage.player.is_playing() {
        let clock = stage.player.position();
        for ((slot, notes), next) in parts.iter().zip(&mut next) {
            let until = notes.first_at_or_after(clock + AHEAD_SECONDS);
            for (index, note) in (*next..until).zip(&notes.as_slice()[*next..until]) {
                let index = u32::try_from(index).expect("a song of under four billion notes");
                let place = stage
                    .player
                    .hold_note(*slot, index, *note)
                    .expect("the notes of a few seconds find a place");
                held.push((note.silent_at(), place));
            }
            *next = until;
        }
        held.retain(|(silent_at, place)| {
            let rings = f64::from(*silent_at) > clock;
            if !rings {
                stage.player.release_note(*place);
            }
            rings
        });
        sound.extend(stage.render(0.25));
    }
    sound.extend(stage.render(1.0));
    sound
}

#[test]
fn the_whole_piece_stays_under_full_scale() {
    let song = clair_de_lune();
    let stage = Stage::new();
    stage.player.set_length(song.seconds());
    let sound = perform(&stage, &song);
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
fn the_keys_down_follow_the_held_notes_and_their_transposition() {
    let stage = Stage::new();
    let held = stage.hold(
        &one_note(1.0, 60),
        Voicing {
            transpose: 2,
            ..pure()
        },
    );
    assert!(stage.player.keys_down_at(0.9).is_empty());
    let down = stage.player.keys_down_at(1.1);
    assert!(down.contains(62) && down.len() == 1);
    assert!(stage.player.keys_down_at(1.3).is_empty());

    stage.player.release_note(held.places[0]);
    assert!(
        stage.player.keys_down_at(1.1).is_empty(),
        "a note nothing holds keeps no key down"
    );
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
    open_sound(&mut robot);
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
    open_sound(&mut robot);
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

// One long A above middle C: it starts at one second of the song and its
// string rings for four.
fn long_note() -> Notes {
    Notes::new(vec![Note {
        start: 1.0,
        held: 4.0,
        ring: 4.0,
        key: 69,
        velocity: 100,
    }])
}

// The level of the A after the first moments of `sound`, past the attack.
fn settled_level(sound: &[f32]) -> f32 {
    level_at(&sound[2_400..], 440.0)
}

fn stage_with(notes: &Notes) -> Stage {
    let stage = Stage::new();
    stage.player.set_length(10.0);
    stage.hold(notes, pure());
    stage
}

#[test]
fn a_clock_dragged_into_a_note_sounds_it_as_far_as_it_has_faded() {
    let at_the_start = stage_with(&long_note());
    at_the_start.player.drag_to(1.0);
    let fresh = settled_level(&at_the_start.render(0.3));
    assert!(fresh > 0.01, "the note sounds under the clock: {fresh}");

    // The pure tone falls 60 dB in five seconds: two seconds in, 24 dB.
    let two_seconds_in = stage_with(&long_note());
    two_seconds_in.player.drag_to(3.0);
    let faded = settled_level(&two_seconds_in.render(0.3)) / fresh;
    assert!(
        (0.045..0.08).contains(&faded),
        "two seconds into the note it sounds at {faded} of its strike"
    );
}

#[test]
fn a_dragged_clock_sounds_each_note_it_enters_and_damps_the_one_it_leaves() {
    let stage = stage_with(&Notes::new(vec![
        Note {
            start: 1.0,
            held: 0.5,
            ring: 0.5,
            key: 69,
            velocity: 100,
        },
        Note {
            start: 2.0,
            held: 0.5,
            ring: 0.5,
            key: 81,
            velocity: 100,
        },
    ]));
    stage.player.drag_to(0.5);
    assert_eq!(first_sound(&stage.render(0.2)), None, "no note at the rest");

    stage.player.drag_to(1.1);
    let first = stage.render(0.3);
    assert!(level_at(&first, 440.0) > 20.0 * level_at(&first, 880.0));

    stage.player.drag_to(2.1);
    stage.render(1.0);
    let second = stage.render(0.3);
    assert!(level_at(&second, 880.0) > 20.0 * level_at(&second, 440.0));

    // Back between the two notes: the clock stands in neither.
    stage.player.drag_to(1.8);
    stage.render(4.0);
    assert_eq!(first_sound(&stage.render(0.2)), None);
}

#[test]
fn a_clock_the_hand_lets_go_of_falls_silent() {
    let stage = stage_with(&long_note());
    stage.player.drag_to(1.5);
    assert!(settled_level(&stage.render(0.3)) > 0.01);
    stage.player.end_drag();
    stage.render(4.0);
    assert_eq!(first_sound(&stage.render(0.2)), None);
}

#[test]
fn a_seek_into_a_note_while_the_piece_plays_sounds_the_note() {
    let stage = stage_with(&long_note());
    stage.player.play();
    stage.render(0.5);
    stage.player.seek(3.0);
    let level = settled_level(&stage.render(0.3));
    assert!(level > 0.0005, "the note sounds from its middle: {level}");
    assert!((stage.player.position() - 3.3).abs() < 1.0e-6);
}

#[test]
fn play_takes_over_from_a_dragged_clock() {
    let stage = stage_with(&one_note(2.0, 60));
    stage.player.drag_to(1.5);
    stage.render(0.1);
    stage.player.play();
    let sound = stage.render(1.0);
    assert_eq!(first_sound(&sound), Some(24_000));
}

// The seconds the tab's clock shows.
fn shown_clock(robot: &mut RobotTestRule<TestRenderer>) -> u32 {
    let texts = robot.get_all_text();
    let clock = texts
        .iter()
        .find_map(|text| text.split_once(" / "))
        .expect("the tab shows its clock")
        .0;
    let (minutes, seconds) = clock.split_once(':').expect("minutes and seconds");
    let whole = |digits: &str| digits.parse::<u32>().expect("the clock shows digits");
    whole(minutes) * 60 + whole(seconds)
}

// Lets the roll rest until the tab takes the hand to have let go.
fn rest_the_roll(robot: &mut RobotTestRule<TestRenderer>, output: &KeptOutput) {
    play_for(robot, output, 0.5);
}

// Pulls the roll down by `pixels` with a held pointer, a step each frame,
// and returns the sound of those frames. The pointer stays down.
fn pull_roll_down(
    robot: &mut RobotTestRule<TestRenderer>,
    output: &KeptOutput,
    pixels: f32,
) -> Vec<f32> {
    let roll = robot
        .find_by_tag("MusicRoll")
        .bounds()
        .expect("the tab shows a roll");
    let x = roll.x + roll.width * 0.5;
    let top = roll.y + roll.height * 0.25;
    robot.mouse_move(x, top);
    robot.mouse_down();
    let mut sound = Vec::new();
    let steps = 30;
    for step in 1..=steps {
        robot.mouse_move(x, top + pixels * step as f32 / steps as f32);
        robot.advance_time(1_000_000_000 / 60);
        if output.renderer.borrow().is_some() {
            sound.extend(output.render(1.0 / 60.0));
        }
    }
    sound
}

#[test]
fn a_hand_that_pulls_the_roll_sounds_the_notes_that_cross_the_line() {
    let _alone = alone();
    let output = KeptOutput::new();
    let mut robot = music_tab(&output);
    assert!(shows(&mut robot, "0:00 / 4:"));

    // Three seconds of the song come down to the line, less the first
    // pixels, which tell a drag from a press.
    let sound = pull_roll_down(&mut robot, &output, 360.0);
    assert!(
        first_sound(&sound).is_some(),
        "the notes that crossed the line sounded"
    );
    let under_the_hand = shown_clock(&mut robot);
    assert!(
        (2..=3).contains(&under_the_hand),
        "the clock went with the roll: {under_the_hand} s"
    );
    assert!(shows(&mut robot, "Play"), "the piece itself does not run");

    // The release flings the roll on; the song goes with it, then rests.
    robot.mouse_up();
    play_for(&mut robot, &output, 4.0);
    rest_the_roll(&mut robot, &output);
    robot.wait_for_idle();
    assert!(shown_clock(&mut robot) >= under_the_hand);
    assert!(shows(&mut robot, "Play"));
    output.render(4.0);
    assert_eq!(
        first_sound(&output.render(0.5)),
        None,
        "the roll rests: the dampers fell"
    );
}

#[test]
fn a_piece_that_plays_runs_on_from_where_a_hand_left_the_roll() {
    let _alone = alone();
    let output = KeptOutput::new();
    let mut robot = music_tab(&output);
    assert!(robot.find_by_tag("MusicRoll").exists());
    assert!(robot.find_by_text("Play").click());
    play_for(&mut robot, &output, 1.0);

    // Ten seconds of the song go by under the hand.
    pull_roll_down(&mut robot, &output, 1200.0);
    assert!(
        shows(&mut robot, "Pause"),
        "the piece still counts as playing"
    );
    robot.mouse_up();
    play_for(&mut robot, &output, 1.5);
    assert!(shows(&mut robot, "Pause"));
    let after_the_hand = shown_clock(&mut robot);
    assert!(
        after_the_hand >= 11,
        "the piece runs on from the place of the roll: {after_the_hand} s"
    );
    // The fling has come to rest, and the clock runs at the pace of the piece.
    play_for(&mut robot, &output, 6.0);
    rest_the_roll(&mut robot, &output);
    let rested = shown_clock(&mut robot);
    play_for(&mut robot, &output, 2.0);
    assert!(shows(&mut robot, "Pause"));
    assert!((1..=3).contains(&(shown_clock(&mut robot) - rested)));
}

// A whole key event of a computer's keyboard through the shell.
fn key(robot: &mut RobotTestRule<TestRenderer>, event: &KeyEvent) -> bool {
    let taken = robot.shell_mut().on_key_event(event);
    robot.advance_time(1_000_000_000 / 60);
    taken
}

#[test]
fn the_home_row_plays_the_piano_and_the_space_bar_the_piece() {
    let _alone = alone();
    let output = KeptOutput::new();
    let mut robot = music_tab(&output);
    open_sound(&mut robot);
    assert!(robot.find_by_text("Pure tone").click());
    robot.wait_for_idle();

    // The A key is middle C.
    assert!(key(&mut robot, &KeyEvent::key_down(Key::A, "a")));
    assert!(
        key(&mut robot, &KeyEvent::key_down(Key::A, "a")),
        "a held key repeats"
    );
    let held = play_for(&mut robot, &output, 0.5);
    let middle_c = level_at(&held, 261.63);
    assert!(middle_c > 0.01, "middle C sounds: {middle_c}");
    assert!(middle_c > 20.0 * level_at(&held, 277.18));
    assert!(key(&mut robot, &KeyEvent::key_up(Key::A)));
    play_for(&mut robot, &output, 4.0);
    assert_eq!(first_sound(&play_for(&mut robot, &output, 0.2)), None);

    assert!(key(&mut robot, &KeyEvent::key_down(Key::Spacebar, " ")));
    play_for(&mut robot, &output, 0.5);
    assert!(shows(&mut robot, "Pause"), "the space bar starts the piece");
    assert!(key(&mut robot, &KeyEvent::key_down(Key::Spacebar, " ")));
    play_for(&mut robot, &output, 0.2);
    robot.wait_for_idle();
    assert!(shows(&mut robot, "Play"), "and stops it");

    assert!(key(
        &mut robot,
        &KeyEvent::key_down(Key::DirectionRight, "")
    ));
    robot.wait_for_idle();
    assert_eq!(
        shown_clock(&mut robot),
        5,
        "an arrow moves the piece by five seconds"
    );
}
