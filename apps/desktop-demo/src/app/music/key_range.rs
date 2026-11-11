// The keys a keyboard shows and where each of them lies along its width. The
// piano roll places its notes with the same numbers, so a falling note lands
// on its key.

// The share of a white key's width a black key takes.
const BLACK_KEY_WIDTH: f32 = 0.6;
// The share of the keyboard's height a black key takes, from the top.
const BLACK_KEY_HEIGHT: f32 = 0.62;
// The share of a white key's width a falling note takes.
const NOTE_WIDTH: f32 = 0.62;

pub fn is_black_key(key: u8) -> bool {
    matches!(key % 12, 1 | 3 | 6 | 8 | 10)
}

// White keys between MIDI key 0 and `key`.
fn white_keys_below_from_zero(key: u8) -> u8 {
    const BELOW_IN_OCTAVE: [u8; 12] = [0, 1, 1, 2, 2, 3, 4, 4, 5, 5, 6, 6];
    key / 12 * 7 + BELOW_IN_OCTAVE[usize::from(key % 12)]
}

// The keys from `lowest` to `highest`, both of them white.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyRange {
    lowest: u8,
    highest: u8,
}

impl KeyRange {
    // The 88 keys of a piano, from the lowest A to the highest C.
    pub const PIANO: Self = Self {
        lowest: 21,
        highest: 108,
    };

    pub fn keys(self) -> impl Iterator<Item = u8> {
        self.lowest..=self.highest
    }

    fn contains(self, key: u8) -> bool {
        (self.lowest..=self.highest).contains(&key)
    }

    // White keys between the lowest key of the range and `key`.
    fn white_keys_below(self, key: u8) -> f32 {
        f32::from(white_keys_below_from_zero(key) - white_keys_below_from_zero(self.lowest))
    }

    fn white_key_width(self, width: f32) -> f32 {
        width / self.white_keys_below(self.highest + 1)
    }

    // The left edge and the width of `key` on a keyboard `width` wide, or
    // `None` for a key outside the range.
    pub fn key_span(self, key: u8, width: f32) -> Option<(f32, f32)> {
        if !self.contains(key) {
            return None;
        }
        let white = self.white_key_width(width);
        let left = self.white_keys_below(key) * white;
        Some(if is_black_key(key) {
            (
                left - white * BLACK_KEY_WIDTH * 0.5,
                white * BLACK_KEY_WIDTH,
            )
        } else {
            (left, white)
        })
    }

    // The left edge and the width of a falling note of `key`.
    pub fn note_span(self, key: u8, width: f32) -> Option<(f32, f32)> {
        let (left, key_width) = self.key_span(key, width)?;
        let note_width = self.white_key_width(width) * NOTE_WIDTH;
        Some((left + (key_width - note_width) * 0.5, note_width))
    }

    // The share of the keyboard's height its black keys take.
    pub fn black_key_height(self) -> f32 {
        BLACK_KEY_HEIGHT
    }

    // The key under a point `x` along a keyboard `width` wide and `y_share`
    // of its height down from the top.
    pub fn key_at(self, x: f32, y_share: f32, width: f32) -> Option<u8> {
        if !(0.0..width).contains(&x) {
            return None;
        }
        let under = |key: &u8| {
            self.key_span(*key, width)
                .is_some_and(|(left, key_width)| (left..left + key_width).contains(&x))
        };
        let black = self.keys().filter(|key| is_black_key(*key)).find(under);
        black
            .filter(|_| y_share < BLACK_KEY_HEIGHT)
            .or_else(|| self.keys().filter(|key| !is_black_key(*key)).find(under))
    }
}
