// The Music tab: composables that draw a score and sound it. The idea of a
// composition as a score comes from DaKapo (https://github.com/O-O-Balance/DaKapo).

mod clair_de_lune;
mod key_range;
mod key_set;
mod keyboard;
mod midi;
mod part;
mod performance;
mod player;
mod reverb;
mod song;
mod synth;
mod tab;
mod timbre;
mod transport;
mod voice;
mod voicing;

pub use clair_de_lune::clair_de_lune;
pub use key_range::{is_black_key, KeyRange};
pub use key_set::KeySet;
pub use keyboard::{Keyboard, KeyboardColors};
pub use midi::read_midi;
pub use part::Part;
pub use performance::{
    local_music_output, local_performance, rememberPerformance, Perform, Performance,
};
pub use player::{MusicOutput, MusicPlayer, PartSlot};
pub use song::{Note, Notes, Song, SongPart};
pub(crate) use tab::MusicTab;
pub use timbre::Timbre;
pub use voicing::{local_voicing, Instrument, Transpose, Voicing, Volume};
