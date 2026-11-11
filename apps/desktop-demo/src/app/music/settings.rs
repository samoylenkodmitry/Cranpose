// What the listener sets for a performance: how fast and loud it goes, how
// it sounds and which of its parts play.

use cranpose_core::{mutableStateOf, remember, MutableState};
use cranpose_ui::composable;

use super::timbre::Timbre;

pub(super) const SLOWEST_TEMPO: f32 = 0.5;
pub(super) const FASTEST_TEMPO: f32 = 2.0;
pub(super) const TRANSPOSE_LIMIT: i8 = 12;
const START_VOLUME: f32 = 0.8;

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Settings {
    // Song seconds that pass in one second of sound.
    pub tempo: MutableState<f32>,
    pub volume: MutableState<f32>,
    // From 0, the first part alone, to 1, the second part alone.
    pub balance: MutableState<f32>,
    pub timbre: MutableState<Timbre>,
    // Semitones every part sounds above its written keys.
    pub transpose: MutableState<i8>,
    // One bit for each part of the song the listener turned off.
    pub silenced: MutableState<u32>,
    // Whether the panel shows more than the transport. The glass of the
    // panel blurs the notes that move behind it each frame, so it stays
    // small until the listener asks for more.
    pub shown: MutableState<bool>,
}

impl Settings {
    // The volume of the part at `index` under the balance. Both of the first
    // two parts play in full at the middle.
    pub(super) fn part_volume(self, index: usize) -> f32 {
        let balance = self.balance.get();
        match index {
            0 => (2.0 * (1.0 - balance)).min(1.0),
            1 => (2.0 * balance).min(1.0),
            _ => 1.0,
        }
    }

    pub(super) fn plays(self, index: usize) -> bool {
        self.silenced.get() & (1 << index) == 0
    }
}

#[composable]
pub(super) fn rememberSettings() -> Settings {
    remember(|| Settings {
        tempo: mutableStateOf(1.0),
        volume: mutableStateOf(START_VOLUME),
        balance: mutableStateOf(0.5),
        timbre: mutableStateOf(Timbre::Piano),
        transpose: mutableStateOf(0),
        silenced: mutableStateOf(0),
        shown: mutableStateOf(false),
    })
    .with(|settings| *settings)
}
