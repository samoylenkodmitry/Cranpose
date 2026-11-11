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
use cranpose_ui::composable;

use super::{
    key_set::KeySet,
    player::{MusicOutput, MusicPlayer},
    voicing::Voicing,
};
use crate::app::ticking::seconds_between;

// The share of its distance to the sound the shown clock closes each frame.
// The synth reports its clock once a block of frames, and the picture moves
// once a frame of the display: the shown clock runs on its own and leans
// toward the reported one.
const PULL: f64 = 0.1;
// Past this distance the shown clock jumps: the song was moved.
const JUMP_SECONDS: f64 = 0.25;
// How long the output device stays on after the clock stops, for the last
// notes and their room to fade.
const REST_AFTER: Duration = Duration::from_secs(5);
const HAND_VELOCITY: u8 = 96;

#[derive(Clone, Copy, PartialEq)]
struct PerformanceState {
    playing: MutableState<bool>,
    shown_seconds: MutableState<f64>,
    // Counts the keys struck by hand, so the device stays awake for them.
    hand_strikes: MutableState<u32>,
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

    // Whether the clock runs. A composable that calls this composes again
    // when the answer changes.
    pub fn is_playing(&self) -> bool {
        self.state.playing.get()
    }

    // Song seconds the listener hears now. Read in a draw block, this
    // redraws the block each frame the clock moves.
    pub fn seconds(&self) -> f64 {
        self.state.shown_seconds.get()
    }

    pub fn play(&self) {
        self.player.play();
        self.state.playing.set(true);
    }

    pub fn pause(&self) {
        self.player.pause();
        self.state.playing.set(false);
    }

    pub fn seek(&self, seconds: f64) {
        self.player.seek(seconds);
        self.state.shown_seconds.set(seconds.max(0.0));
    }

    // The keys the parts hold down at the shown clock.
    pub fn keys_down(&self) -> KeySet {
        self.player.keys_down_at(self.seconds())
    }

    pub fn key_down(&self, key: u8, voicing: Voicing) {
        self.player.key_down(key, HAND_VELOCITY, voicing);
        self.state.hand_strikes.update(|strikes| *strikes += 1);
    }

    pub fn key_up(&self, key: u8) {
        self.player.key_up(key);
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
        let shown = self.state.shown_seconds.get() + f64::from(frame_seconds) * tempo;
        let apart = heard - shown;
        let shown = if apart.abs() > JUMP_SECONDS {
            heard
        } else {
            shown + apart * PULL
        };
        self.state.shown_seconds.set(shown.max(0.0));
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
            hand_strikes: mutableStateOf(0),
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
    let playing = performance.is_playing();
    let hand_strikes = performance.state.hand_strikes.get();
    {
        let player = performance.player.clone();
        LaunchedEffect((playing, hand_strikes), async move |_| {
            if !playing {
                delay(REST_AFTER).await;
                player.rest();
            }
        });
    }
    {
        let performance = performance.clone();
        LaunchedEffect(playing, async move |scope| {
            if !playing {
                return;
            }
            let mut last = scope.with_frame_nanos(|now| now).await;
            loop {
                let now = scope.with_frame_nanos(|now| now).await;
                performance.follow(seconds_between(last, now));
                last = now;
            }
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
