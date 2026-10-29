use crate::line_breaker::{BreakKind, BreakOpportunity};

// The line break classes of UAX #14 that ASCII characters take.
#[derive(Clone, Copy, Debug)]
enum Class {
    Alphabetic,
    BreakAfter,
    ClosePunctuation,
    CloseParenthesis,
    CombiningMark,
    CarriageReturn,
    Exclamation,
    Hyphen,
    InfixSeparator,
    LineFeed,
    MandatoryBreak,
    Numeric,
    OpenPunctuation,
    PostfixNumeric,
    PrefixNumeric,
    Quotation,
    Space,
    Symbol,
}

use Class::{
    Alphabetic, BreakAfter, CarriageReturn, CloseParenthesis, ClosePunctuation, CombiningMark,
    Exclamation, Hyphen, InfixSeparator, LineFeed, MandatoryBreak, Numeric, OpenPunctuation,
    PostfixNumeric, PrefixNumeric, Quotation, Space, Symbol,
};

// The class of each ASCII character, as `LineBreak.txt` gives it.
const fn class(byte: u8) -> Class {
    match byte {
        b'\t' | b'|' => BreakAfter,
        b'\n' => LineFeed,
        0x0B | 0x0C => MandatoryBreak,
        b'\r' => CarriageReturn,
        0x00..=0x1F | 0x7F => CombiningMark,
        b' ' => Space,
        b'!' | b'?' => Exclamation,
        b'"' | b'\'' => Quotation,
        b'$' | b'+' | b'\\' => PrefixNumeric,
        b'%' => PostfixNumeric,
        b'(' | b'[' | b'{' => OpenPunctuation,
        b')' | b']' => CloseParenthesis,
        b'}' => ClosePunctuation,
        b',' | b'.' | b':' | b';' => InfixSeparator,
        b'-' => Hyphen,
        b'/' => Symbol,
        b'0'..=b'9' => Numeric,
        _ => Alphabetic,
    }
}

// Every class, in the order the enum declares them.
const CLASSES: [Class; 18] = [
    Alphabetic,
    BreakAfter,
    ClosePunctuation,
    CloseParenthesis,
    CombiningMark,
    CarriageReturn,
    Exclamation,
    Hyphen,
    InfixSeparator,
    LineFeed,
    MandatoryBreak,
    Numeric,
    OpenPunctuation,
    PostfixNumeric,
    PrefixNumeric,
    Quotation,
    Space,
    Symbol,
];

const _: () = {
    let mut index = 0;
    while index < CLASSES.len() {
        assert!(CLASSES[index] as usize == index);
        index += 1;
    }
};

// What stands before a character, which tells whether a line may break
// before it.
#[derive(Clone, Copy)]
struct Before {
    // The class of the character before, a combining mark taking its
    // base's (LB9).
    class: Class,
    // Whether an open punctuation stands before, with spaces after it or
    // not (LB14).
    opened: bool,
}

const STATES: usize = CLASSES.len() * 2;

// How a step ends a line: not at all, between words, or after a mandatory
// break, in the top two bits of a step; the low six hold the state after
// it.
const SOFT: u8 = 1 << 6;
const HARD: u8 = 2 << 6;
const STATE: u8 = (1 << 6) - 1;

// The class of each ASCII character.
const CLASS_OF: [u8; 128] = {
    let mut table = [0; 128];
    let mut byte = 0;
    while byte < table.len() {
        table[byte] = class(byte as u8) as u8;
        byte += 1;
    }
    table
};

// For each state and the class of the character after it, whether a line
// breaks before that character and the state after it, from the rules.
const STEPS: [[u8; CLASSES.len()]; STATES] = {
    let mut table = [[0; CLASSES.len()]; STATES];
    let mut state = 0;
    while state < STATES {
        let before = Before::of(state);
        let mut right = 0;
        while right < CLASSES.len() {
            table[state][right] =
                before.breaks(CLASSES[right]) | before.next(CLASSES[right]).index() as u8;
            right += 1;
        }
        state += 1;
    }
    table
};

// Replaces `breaks` with the line break opportunities of ASCII `text` as the
// segmenter finds them, without its tables: the rules of UAX #14 for the
// classes ASCII characters take, as the segmenter applies them.
pub(crate) fn ascii_breaks(text: &[u8], breaks: &mut Vec<BreakOpportunity>) {
    breaks.clear();
    let Some(&first) = text.first() else {
        return;
    };
    let start = alone(class(first));
    let mut state = Before {
        class: start,
        opened: matches!(start, OpenPunctuation),
    }
    .index();
    for (at, &byte) in text.iter().enumerate().skip(1) {
        let step = STEPS[state][usize::from(CLASS_OF[usize::from(byte & 0x7F)])];
        state = usize::from(step & STATE);
        let kind = match step & !STATE {
            SOFT => BreakKind::Soft,
            HARD => BreakKind::Hard,
            _ => continue,
        };
        breaks.push(BreakOpportunity { offset: at, kind });
    }
    let kind = if hard(Before::of(state).class) {
        BreakKind::Hard
    } else {
        BreakKind::Soft
    };
    breaks.push(BreakOpportunity {
        offset: text.len(),
        kind,
    });
}

const fn hard(class: Class) -> bool {
    matches!(class, MandatoryBreak | CarriageReturn | LineFeed)
}

// The class of a character with nothing before it that it joins: a
// combining mark counts as alphabetic (LB10).
const fn alone(class: Class) -> Class {
    if matches!(class, CombiningMark) {
        Alphabetic
    } else {
        class
    }
}

impl Before {
    const fn index(self) -> usize {
        self.class as usize * 2 + self.opened as usize
    }

    const fn of(index: usize) -> Self {
        Self {
            class: CLASSES[index / 2],
            opened: index % 2 == 1,
        }
    }

    // How a line breaks before a character of class `right`: not at all,
    // `SOFT` or `HARD`.
    const fn breaks(self, right: Class) -> u8 {
        let left = self.class;
        // LB4, LB5: after a mandatory break, but not inside CR LF.
        if matches!((left, right), (CarriageReturn, LineFeed)) {
            return 0;
        }
        if hard(left) {
            return HARD;
        }
        // LB6, LB7, LB9: before a mandatory break, a space or a mark.
        if hard(right)
            || matches!(right, Space)
            || (matches!(right, CombiningMark) && !matches!(left, Space))
        {
            return 0;
        }
        if self.joins(alone(right)) { 0 } else { SOFT }
    }

    // Whether the rules from LB13 on keep a character of class `right`
    // after this.
    const fn joins(self, right: Class) -> bool {
        let left = self.class;
        // LB13, LB14.
        if self.opened
            || matches!(
                right,
                ClosePunctuation | CloseParenthesis | Exclamation | InfixSeparator | Symbol
            )
        {
            return true;
        }
        // LB18.
        if matches!(left, Space) {
            return false;
        }
        // LB19, LB21.
        if matches!(left, Quotation) || matches!(right, Quotation | BreakAfter | Hyphen) {
            return true;
        }
        matches!(
            (left, right),
            // LB23, LB24, LB28, LB29, LB30.
            (
                Alphabetic,
                Numeric | PrefixNumeric | PostfixNumeric | Alphabetic | OpenPunctuation,
            ) | (Numeric | PrefixNumeric | PostfixNumeric | InfixSeparator, Alphabetic)
                | (Numeric, OpenPunctuation)
                | (CloseParenthesis, Alphabetic | Numeric)
                // LB25, in the pairs the segmenter takes.
                | (ClosePunctuation | CloseParenthesis | Numeric, PrefixNumeric | PostfixNumeric)
                | (PrefixNumeric | PostfixNumeric, OpenPunctuation | Numeric)
                | (Hyphen | InfixSeparator | Numeric | Symbol, Numeric)
        )
    }

    // What stands before the character after one of class `right`.
    const fn next(self, right: Class) -> Self {
        if matches!(right, CombiningMark) && !hard(self.class) && !matches!(self.class, Space) {
            return self;
        }
        let right = alone(right);
        Self {
            class: right,
            opened: matches!(right, OpenPunctuation) || (matches!(right, Space) && self.opened),
        }
    }
}

#[cfg(test)]
#[path = "tests/ascii_breaks_tests.rs"]
mod tests;
