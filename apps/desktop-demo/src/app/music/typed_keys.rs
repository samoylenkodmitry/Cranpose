// The keys of a computer's keyboard that play the piano: the home row is
// the white keys from middle C, and the row above it holds the black keys
// between them.

use cranpose_ui::Key;

const MIDDLE_C: u8 = 60;

// The piano key a key of the computer plays, or none.
pub(super) fn piano_key(key: Key) -> Option<u8> {
    let above_middle_c = match key {
        Key::A => 0,
        Key::W => 1,
        Key::S => 2,
        Key::E => 3,
        Key::D => 4,
        Key::F => 5,
        Key::T => 6,
        Key::G => 7,
        Key::Y => 8,
        Key::H => 9,
        Key::U => 10,
        Key::J => 11,
        Key::K => 12,
        Key::O => 13,
        Key::L => 14,
        Key::P => 15,
        Key::Semicolon => 16,
        _ => return None,
    };
    Some(MIDDLE_C + above_middle_c)
}
