// Debussy's "Clair de lune" as sequenced by Bernd Krueger
// (http://www.piano-midi.de), who licenses the file under CC BY-SA 3.0 DE.
// `assets/licenses/README.md` carries the notice.

use super::{midi::read_midi, song::Song};

const MIDI: &[u8] = include_bytes!("../../../assets/music/clair-de-lune.mid");

pub(super) const TITLE: &str = "Clair de lune";
pub(super) const COMPOSER: &str = "Claude Debussy";
pub(super) const CREDIT: &str = "MIDI sequence by Bernd Krueger, piano-midi.de, CC BY-SA 3.0 DE";

pub fn clair_de_lune() -> Song {
    read_midi(MIDI).expect("the bundled MIDI file reads")
}
