// A performance in the composition: the player, and the clock the picture
// follows. `Perform` gives it to the parts and keyboards inside its content.

use std::{cell::RefCell, time::Duration};

use cranpose_audio::backend::RENDER_LEAD_SECONDS;
use cranpose_core::{
    compositionLocalOf, delay, mutableStateOf, remember, CompositionLocal,
    CompositionLocalProvider, DisposableEffect, DisposableEffectResult, LaunchedEffect,
    MutableState,
};
use cranpose_services::{rememberLifecycleState, LifecycleState};
use cranpose_ui::{composable, Color};

use super::{
    held_notes::{KeyDown, NotePlace, PartSlot},
    key_set::KeySet,
    player::{MusicOutput, MusicPlayer},
    song::{Note, Notes},
    voicing::Voicing,
};
use crate::app::ticking::Ticking;

// The share of its distance to the sound the shown clock closes each frame.
// The synth reports its clock once a block of frames, and the picture moves
// once a frame of the display: the shown clock runs on its own and leans
// toward the reported one.
const PULL: f64 = 0.1;
// Past this distance the shown clock jumps: the song was moved.
const JUMP_SECONDS: f64 = 0.25;
// A step of the shown clock under this is no step: a clock whose sound
// stands still comes to rest, and the picture with it.
const STILL_SECONDS: f64 = 1.0e-4;
// How long the output device stays on after the clock stops, for the last
// notes and their room to fade.
const REST_AFTER: Duration = Duration::from_secs(5);
const HAND_VELOCITY: u8 = 96;

// A clock under a hand.
#[derive(Clone, Copy, PartialEq)]
struct Drag {
    // Whether the piece ran when the hand took the clock, and runs on when
    // the hand lets go.
    resumes: bool,
}

#[derive(Clone, Copy, PartialEq)]
struct PerformanceState {
    playing: MutableState<bool>,
    shown_seconds: MutableState<f64>,
    drag: MutableState<Option<Drag>>,
    // Counts the moves of a dragged clock, so a watcher can tell when the
    // hand rests.
    drag_moves: MutableState<u32>,
    // Counts the keys struck by hand and the moves of a dragged clock, so
    // the device stays awake for them.
    wakes: MutableState<u32>,
    // Counts the holds and releases of parts and notes, so the lights of
    // the keys follow them.
    holds: MutableState<u32>,
    hand_keys: MutableState<KeySet>,
}

// A song being played: its player and the clock its picture shows.
#[derive(Clone, PartialEq)]
pub struct Performance {
    player: MusicPlayer,
    state: PerformanceState,
}

impl Performance {
    pub fn player(&self) -> &MusicPlayer {
        &self.player
    }

    // Whether the piece plays: its clock runs, or a hand holds the clock
    // of a running piece for a moment. A composable that calls this
    // composes again when the answer changes.
    pub fn is_playing(&self) -> bool {
        self.state.playing.get() || self.state.drag.get().is_some_and(|drag| drag.resumes)
    }

    // Song seconds the listener hears now. Read in a draw block, this
    // redraws the block each frame the clock moves.
    pub fn seconds(&self) -> f64 {
        self.state.shown_seconds.get()
    }

    // The same seconds for a scope that something else runs again:
    // this read subscribes no one.
    pub fn seconds_now(&self) -> f64 {
        self.state.shown_seconds.get_non_reactive()
    }

    pub fn play(&self) {
        self.state.drag.set(None);
        self.player.play();
        self.state.playing.set(true);
    }

    // Stops a piece that plays; starts one that does not, from its start
    // when it has ended.
    pub fn toggle(&self) {
        if self.is_playing() {
            self.pause();
            return;
        }
        if self.seconds() >= f64::from(self.player.length()) {
            self.seek(0.0);
        }
        self.play();
    }

    pub fn pause(&self) {
        if self.state.drag.get().is_some() {
            self.state.drag.set(None);
            self.player.end_drag();
        }
        self.player.pause();
        self.state.playing.set(false);
    }

    // Drags the clock to `seconds` of the song by hand. The notes the
    // clock stands in and enters sound; a running piece waits for
    // `end_drag`.
    pub fn drag_to(&self, seconds: f64) {
        let resumes = match self.state.drag.get() {
            Some(drag) => drag.resumes,
            None => self.state.playing.get(),
        };
        self.state.drag.set(Some(Drag { resumes }));
        self.state.playing.set(false);
        self.player.drag_to(seconds);
        self.state.shown_seconds.set(seconds.max(0.0));
        self.state.drag_moves.update(|moves| *moves += 1);
        self.state.wakes.update(|wakes| *wakes += 1);
    }

    // Lets go of a dragged clock: a piece that ran runs on from where the
    // hand left the clock, and the notes of one that stood fall silent.
    pub fn end_drag(&self) {
        match self.state.drag.get() {
            Some(Drag { resumes: true }) => self.play(),
            Some(Drag { resumes: false }) => {
                self.state.drag.set(None);
                self.player.end_drag();
            }
            None => {}
        }
    }

    // Lets go of a dragged clock when the piece ran before the hand took it.
    pub fn end_running_drag(&self) {
        if self.state.drag.get().is_some_and(|drag| drag.resumes) {
            self.play();
        }
    }

    // Whether a hand drags the clock.
    pub fn is_dragged(&self) -> bool {
        self.state.drag.get().is_some()
    }

    // Whether a hand drags the clock, and the count of its moves. A
    // composable that calls this composes again with each move.
    pub fn drag_moves(&self) -> Option<u32> {
        self.state.drag.get().map(|_| self.state.drag_moves.get())
    }

    // Holds the part that `notes` belong to, shown in `color`, until
    // `release_part`.
    pub fn hold_part(&self, notes: &Notes, color: Color) -> Option<PartSlot> {
        let slot = self.player.hold_part(notes, color);
        self.state.holds.update(|holds| *holds += 1);
        slot
    }

    pub fn voice_part(&self, slot: PartSlot, voicing: Voicing) {
        self.player.voice_part(slot, voicing);
        self.state.holds.update(|holds| *holds += 1);
    }

    pub fn release_part(&self, slot: PartSlot) {
        self.player.release_part(slot);
        self.state.holds.update(|holds| *holds += 1);
    }

    // Holds `note`, the one at `index` of the part in `slot`, until
    // `release_note`: it sounds while the clock stands in it.
    pub fn hold_note(&self, slot: PartSlot, index: u32, note: Note) -> Option<NotePlace> {
        let place = self.player.hold_note(slot, index, note);
        self.state.holds.update(|holds| *holds += 1);
        place
    }

    pub fn release_note(&self, place: NotePlace) {
        self.player.release_note(place);
        self.state.holds.update(|holds| *holds += 1);
    }

    pub fn seek(&self, seconds: f64) {
        self.player.seek(seconds);
        self.state.shown_seconds.set(seconds.max(0.0));
    }

    // Calls `down` for each key the held notes keep down at the shown
    // clock. A scope that calls this runs again when the clock moves and
    // when a note is held or let go.
    pub fn for_each_key_down(&self, down: impl FnMut(KeyDown)) {
        self.state.holds.get();
        self.player.for_each_key_down(self.seconds(), down);
    }

    // Strikes `key` by hand. A key that is down already stays as it is,
    // as a held key of a computer's keyboard repeats its press.
    pub fn key_down(&self, key: u8, voicing: Voicing) {
        if self.state.hand_keys.get().contains(key) {
            return;
        }
        self.state.hand_keys.update(|keys| keys.insert(key));
        self.player.key_down(key, HAND_VELOCITY, voicing);
        self.state.wakes.update(|wakes| *wakes += 1);
    }

    pub fn key_up(&self, key: u8) {
        self.state.hand_keys.update(|keys| keys.remove(key));
        self.player.key_up(key);
    }

    // The keys a hand holds down. Read in a draw block, this redraws the
    // block when a key goes down or up.
    pub fn hand_keys(&self) -> KeySet {
        self.state.hand_keys.get()
    }

    // Moves the shown clock by a frame that took `frame_seconds`.
    fn follow(&self, frame_seconds: f32) {
        let tempo = f64::from(self.player.tempo());
        let heard = self.player.position() - f64::from(RENDER_LEAD_SECONDS) * tempo;
        if !self.player.is_playing() {
            self.state.playing.set(false);
            self.state.shown_seconds.set(self.player.position());
            return;
        }
        let before = self.state.shown_seconds.get();
        let shown = before + f64::from(frame_seconds) * tempo;
        let apart = heard - shown;
        let shown = if apart.abs() > JUMP_SECONDS {
            heard
        } else {
            shown + apart * PULL
        }
        .max(0.0);
        if (shown - before).abs() >= STILL_SECONDS {
            self.state.shown_seconds.set(shown);
        }
    }
}

pub fn local_performance() -> CompositionLocal<Option<Performance>> {
    thread_local! {
        static LOCAL_PERFORMANCE: RefCell<Option<CompositionLocal<Option<Performance>>>> = const { RefCell::new(None) };
    }
    LOCAL_PERFORMANCE.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| compositionLocalOf(|| None))
            .clone()
    })
}

// The output a performance remembered inside the provider plays through.
// The platform's device, unless a provider gives another.
pub fn local_music_output() -> CompositionLocal<MusicOutput> {
    thread_local! {
        static LOCAL_MUSIC_OUTPUT: RefCell<Option<CompositionLocal<MusicOutput>>> = const { RefCell::new(None) };
    }
    LOCAL_MUSIC_OUTPUT.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| compositionLocalOf(MusicOutput::device))
            .clone()
    })
}

// A performance of a song `seconds` long, kept while the caller stays in
// the composition.
#[composable]
pub fn rememberPerformance(seconds: f32) -> Performance {
    let output = local_music_output().current();
    let performance = remember(|| Performance {
        player: MusicPlayer::new(output),
        state: PerformanceState {
            playing: mutableStateOf(false),
            shown_seconds: mutableStateOf(0.0),
            drag: mutableStateOf(None),
            drag_moves: mutableStateOf(0),
            wakes: mutableStateOf(0),
            holds: mutableStateOf(0),
            hand_keys: mutableStateOf(KeySet::EMPTY),
        },
    })
    .with(Performance::clone);
    performance.player.set_length(seconds);
    performance
}

// Plays the parts and keyboards in `content` as `performance`, and moves
// the clock of the performance each frame it plays.
#[composable]
pub fn Perform(performance: Performance, content: impl FnMut() + 'static) {
    let running = performance.state.playing.get();
    let wakes = performance.state.wakes.get();
    {
        let player = performance.player.clone();
        LaunchedEffect((running, wakes), async move |_| {
            if !running {
                delay(REST_AFTER).await;
                player.rest();
            }
        });
    }
    {
        let performance = performance.clone();
        Ticking(running, move |frame_seconds| {
            performance.follow(frame_seconds);
        });
    }
    // The picture and the sound belong together: the piece stops when the
    // app leaves the screen, as it does when this call leaves the composition.
    let on_screen = !matches!(
        rememberLifecycleState().get(),
        LifecycleState::Stopped | LifecycleState::Destroyed
    );
    {
        let performance = performance.clone();
        DisposableEffect(on_screen, move |scope| {
            if !on_screen {
                return DisposableEffectResult::default();
            }
            scope.on_dispose(move || {
                performance.pause();
                performance.player.rest();
            })
        });
    }
    CompositionLocalProvider([local_performance().provides(Some(performance))], content);
}
