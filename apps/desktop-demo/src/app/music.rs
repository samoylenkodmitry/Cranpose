// The Music tab: composables that draw a score and sound it. The idea of a
// composition as a score comes from DaKapo (https://github.com/O-O-Balance/DaKapo).

mod clair_de_lune;
mod controls;
mod held_notes;
mod key_range;
mod key_set;
mod keyboard;
mod midi;
mod night_sky;
mod note_bar;
mod part;
mod performance;
mod piano_key;
mod player;
mod reverb;
mod roll;
mod settings;
mod song;
mod synth;
mod tab;
mod timbre;
mod transport;
mod typed_keys;
mod voice;
mod voice_bank;
mod voicing;

pub use clair_de_lune::clair_de_lune;
pub use held_notes::{KeyDown, NotePlace, PartSlot};
pub use key_range::{is_black_key, KeyRange};
pub use key_set::KeySet;
pub use keyboard::Keyboard;
pub use midi::read_midi;
pub use note_bar::NoteBar;
pub use part::{local_part, Part, PartScope};
pub use performance::{
    local_music_output, local_performance, rememberPerformance, Perform, Performance,
};
pub use piano_key::{KeyLights, KeyboardColors, PianoKey};
pub use player::{MusicOutput, MusicPlayer};
pub use roll::{local_roll_slice, Roll, RollSlice};
pub use song::{Note, Notes, Song, SongPart};
pub(crate) use tab::MusicTab;
pub use timbre::Timbre;
pub use voicing::{local_voicing, Instrument, Transpose, Voicing, Volume};
