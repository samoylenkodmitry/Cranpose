use crate::line_breaker::{BreakKind, BreakOpportunity};

// The line break classes of UAX #14 that ASCII characters take.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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

// Replaces `breaks` with the line break opportunities of ASCII `text` as the
// segmenter finds them, without its tables: the rules of UAX #14 for the
// classes ASCII characters take, as the segmenter applies them.
pub(crate) fn ascii_breaks(text: &[u8], breaks: &mut Vec<BreakOpportunity>) {
    breaks.clear();
    let Some(&first) = text.first() else {
        return;
    };
    let start = alone(class(first));
    let mut before = Before {
        class: start,
        opened: start == OpenPunctuation,
    };
    for at in 1..text.len() {
        let right = class(text[at]);
        if let Some(kind) = before.breaks(right) {
            breaks.push(BreakOpportunity { offset: at, kind });
        }
        before = before.next(right);
    }
    let kind = if hard(before.class) {
        BreakKind::Hard
    } else {
        BreakKind::Soft
    };
    breaks.push(BreakOpportunity {
        offset: text.len(),
        kind,
    });
}

fn hard(class: Class) -> bool {
    matches!(class, MandatoryBreak | CarriageReturn | LineFeed)
}

// The class of a character with nothing before it that it joins: a
// combining mark counts as alphabetic (LB10).
fn alone(class: Class) -> Class {
    if class == CombiningMark {
        Alphabetic
    } else {
        class
    }
}

impl Before {
    // Whether a line may break before a character of class `right`, and
    // how.
    fn breaks(&self, right: Class) -> Option<BreakKind> {
        let left = self.class;
        // LB4, LB5: after a mandatory break, but not inside CR LF.
        if left == CarriageReturn && right == LineFeed {
            return None;
        }
        if hard(left) {
            return Some(BreakKind::Hard);
        }
        // LB6, LB7, LB9: before a mandatory break, a space or a mark.
        if hard(right) || right == Space || (right == CombiningMark && left != Space) {
            return None;
        }
        (!self.joins(left, alone(right))).then_some(BreakKind::Soft)
    }

    // Whether the rules from LB13 on keep a character of class `right`
    // after one of class `left`.
    fn joins(&self, left: Class, right: Class) -> bool {
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
        if left == Space {
            return false;
        }
        // LB19, LB21.
        if left == Quotation || matches!(right, Quotation | BreakAfter | Hyphen) {
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
    fn next(self, right: Class) -> Self {
        if right == CombiningMark && !hard(self.class) && self.class != Space {
            return self;
        }
        let right = alone(right);
        Self {
            class: right,
            opened: right == OpenPunctuation || (right == Space && self.opened),
        }
    }
}

#[cfg(test)]
#[path = "tests/ascii_breaks_tests.rs"]
mod tests;
