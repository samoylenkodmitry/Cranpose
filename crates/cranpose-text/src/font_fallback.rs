// Which typeface of a fallback chain draws each character of a text.

use std::{
    cmp::Ordering,
    ops::{Range, RangeInclusive},
    sync::LazyLock,
};

use icu_properties::{
    CodePointMapData, CodePointSetData, CodePointSetDataBorrowed,
    props::{DefaultIgnorableCodePoint, EmojiPresentation, GraphemeClusterBreak},
};
use smallvec::SmallVec;

use crate::{coverage::Coverage, typeface::Typeface};

// The typefaces a request draws with, in fallback order, with what the split
// reads of each.
pub(crate) struct FallbackChain<'a> {
    links: SmallVec<[Link<'a>; 8]>,
}

struct Link<'a> {
    // The typeface's index in its resolver.
    typeface: usize,
    coverage: &'a Coverage,
    is_color: bool,
}

impl<'a> FallbackChain<'a> {
    pub(crate) fn new(typefaces: impl Iterator<Item = (usize, &'a Typeface)>) -> Self {
        Self {
            links: typefaces
                .map(|(typeface, face)| Link {
                    typeface,
                    coverage: face.coverage(),
                    is_color: face.is_color(),
                })
                .collect(),
        }
    }

    // Splits `text` into runs code point by code point, as Android's minikin
    // does, and passes each run's byte range and typeface index to `visit`.
    pub(crate) fn split(&self, text: &str, mut visit: impl FnMut(Range<usize>, usize)) {
        let mut split = Split {
            chain: self,
            run: None,
            cluster_start: 0,
            boundaries: ClusterBoundaries {
                previous: GraphemeClusterBreak::Control,
                flag_open: false,
            },
        };
        let mut chars = text.char_indices().peekable();
        while let Some((at, ch)) = chars.next() {
            let joins = split.boundaries.joins(ch);
            match split.run {
                Some(run) if joins => split.extend_cluster(text, at, ch, run, &mut visit),
                run => {
                    let next = chars.peek().map(|&(_, next)| next);
                    split.start_cluster(at, ch, next, run, &mut visit);
                }
            }
        }
        if let Some(run) = split.run {
            visit(run.start..text.len(), self.links[run.link].typeface);
        }
    }

    // The first link that maps every character of `needed`, a color one for
    // emoji and a plain one for text before any other.
    fn pick(&self, needed: &[char], wants_color: bool) -> Option<usize> {
        let mut covering = None;
        for (position, link) in self.links.iter().enumerate() {
            if needed.iter().all(|&ch| link.coverage.contains(ch)) {
                if link.is_color == wants_color {
                    return Some(position);
                }
                covering.get_or_insert(position);
            }
        }
        covering
    }
}

struct Split<'c, 'a> {
    chain: &'c FallbackChain<'a>,
    run: Option<Run>,
    cluster_start: usize,
    boundaries: ClusterBoundaries,
}

#[derive(Clone, Copy)]
struct Run {
    start: usize,
    link: usize,
}

impl Split<'_, '_> {
    // `ch` extends the cluster at `cluster_start` (a mark, a joiner, a
    // variation selector, a Hangul jamo or the second regional indicator of a
    // flag): it stays in the run while the run's typeface draws it, and
    // otherwise the whole cluster moves to a typeface that draws all of it.
    fn extend_cluster(
        &mut self,
        text: &str,
        at: usize,
        ch: char,
        run: Run,
        visit: &mut impl FnMut(Range<usize>, usize),
    ) {
        let links = &self.chain.links;
        if links[run.link].coverage.contains(ch) || needs_no_glyph(ch) {
            return;
        }
        let Some(cluster) = text.get(self.cluster_start..at + ch.len_utf8()) else {
            return;
        };
        let needed: SmallVec<[char; 8]> =
            cluster.chars().filter(|&ch| !needs_no_glyph(ch)).collect();
        if let Some(moved) = self.chain.pick(&needed, wants_color(cluster))
            && moved != run.link
        {
            if run.start < self.cluster_start {
                visit(run.start..self.cluster_start, links[run.link].typeface);
            }
            self.run = Some(Run {
                start: self.cluster_start,
                link: moved,
            });
        }
    }

    // `ch` starts a cluster: it goes to the first link that maps it, except
    // that a character without a glyph of its own, and punctuation the run's
    // typeface has, stay in the run. `next` is the character after `ch`,
    // whose variation selector or keycap asks for a presentation.
    fn start_cluster(
        &mut self,
        at: usize,
        ch: char,
        next: Option<char>,
        run: Option<Run>,
        visit: &mut impl FnMut(Range<usize>, usize),
    ) {
        self.cluster_start = at;
        let ignorable = needs_no_glyph(ch);
        if let Some(run) = run
            && (ignorable || (sticky(ch) && self.chain.links[run.link].coverage.contains(ch)))
        {
            return;
        }
        let wants_color = match next {
            Some('\u{FE0F}' | '\u{20E3}') => true,
            Some('\u{FE0E}') => false,
            _ => asks_presentation(ch),
        };
        let needed = [ch];
        let link = self
            .chain
            .pick(if ignorable { &[] } else { &needed }, wants_color)
            .or_else(|| run.map(|run| run.link))
            .unwrap_or(0);
        if run.is_none_or(|run| run.link != link) {
            if let Some(run) = run {
                visit(run.start..at, self.chain.links[run.link].typeface);
            }
            self.run = Some(Run { start: at, link });
        }
    }
}

// Where grapheme clusters begin, from each character's Grapheme_Cluster_Break
// class: the rules of UAX #29 that keep marks, joiners and what follows a
// joiner, Hangul syllables and flags together.
struct ClusterBoundaries {
    previous: GraphemeClusterBreak,
    // The previous character is a regional indicator that starts a flag.
    flag_open: bool,
}

impl ClusterBoundaries {
    // Whether `ch` belongs to the cluster of the character before it.
    fn joins(&mut self, ch: char) -> bool {
        use GraphemeClusterBreak as Class;
        let class = CodePointMapData::<GraphemeClusterBreak>::new().get(ch);
        let previous = self.previous;
        let joins = matches!(
            class,
            Class::Extend | Class::SpacingMark | Class::ZWJ | Class::EModifier
        ) || matches!(previous, Class::Prepend | Class::ZWJ)
            || (previous == Class::L
                && matches!(class, Class::L | Class::V | Class::LV | Class::LVT))
            || (matches!(previous, Class::LV | Class::V) && matches!(class, Class::V | Class::T))
            || (matches!(previous, Class::LVT | Class::T) && class == Class::T)
            || (self.flag_open && class == Class::RegionalIndicator);
        self.flag_open = class == Class::RegionalIndicator && !joins;
        self.previous = class;
        joins
    }
}

// Whether a typeface with `coverage` draws all of `text` with no fallback:
// it maps each character or the character needs no glyph, and no character
// asks for emoji presentation. ASCII characters, which text in every script
// uses, test one bit of the typeface's ASCII mask, and when the typeface maps
// all of ASCII an ASCII text needs only the vectorized ASCII test.
pub(crate) fn draws_plainly(coverage: &Coverage, text: &str) -> bool {
    let ascii = coverage.ascii() | ASCII_CONTROLS;
    if ascii == u128::MAX && text.is_ascii() {
        return true;
    }
    text.chars().all(|ch| {
        if ch.is_ascii() {
            ascii >> u32::from(ch) & 1 != 0
        } else {
            (coverage.contains(ch) || needs_no_glyph(ch)) && !asks_presentation(ch)
        }
    })
}

// The ASCII control characters, which need no glyph.
const ASCII_CONTROLS: u128 = ((1 << 0x20) - 1) | (1 << 0x7F);

// Punctuation and spaces that stay in the run's typeface when it has them,
// so that punctuation after a fallback word matches the word (Android's
// minikin) and a fallback script's words separated by spaces shape as one
// item. Android takes a space from the first family that has it; keeping it
// in the run instead saves shaping each word apart, which for Devanagari
// walks every lookup of the font once per word.
fn sticky(ch: char) -> bool {
    matches!(
        ch,
        ' ' | '!' | ',' | '-' | '.' | ':' | ';' | '?' | '\u{00A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{2010}' | '\u{2011}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
    )
}

// A control character or a default-ignorable one (joiners, variation
// selectors, bidi controls, tags), which the shaper hides when no font has it.
pub(crate) fn needs_no_glyph(ch: char) -> bool {
    static IGNORABLE: LazyLock<CodePointSet> =
        LazyLock::new(|| CodePointSet::of(CodePointSetData::new::<DefaultIgnorableCodePoint>()));
    ch.is_control() || IGNORABLE.contains(ch)
}

// Whether `ch` decides between emoji and text presentation. Every such
// character lies at or above U+20E3, which keeps most text off the lookup.
fn asks_presentation(ch: char) -> bool {
    static EMOJI_PRESENTATION: LazyLock<CodePointSet> =
        LazyLock::new(|| CodePointSet::of(CodePointSetData::new::<EmojiPresentation>()));
    ch >= '\u{20E3}'
        && (matches!(ch, '\u{20E3}' | '\u{FE0E}' | '\u{FE0F}') || EMOJI_PRESENTATION.contains(ch))
}

// Whether `cluster` shows as emoji: a variation selector 16 or a keycap asks
// for it, a variation selector 15 asks for text, and otherwise the first
// character's default presentation decides.
fn wants_color(cluster: &str) -> bool {
    let mut wants = false;
    for (position, ch) in cluster.chars().enumerate() {
        match ch {
            '\u{FE0E}' => return false,
            '\u{FE0F}' | '\u{20E3}' => wants = true,
            _ if position == 0 => wants = asks_presentation(ch),
            _ => {}
        }
    }
    wants
}

// A Unicode property set for lookups per character. The property data is a
// sorted list of ranges, a binary search per lookup; this keeps the Basic
// Multilingual Plane part, which most text uses, as a bitmap, and the few
// ranges above it as a sorted list, so the bitmap's page index stays small.
struct CodePointSet {
    basic: Coverage,
    astral: Box<[RangeInclusive<u32>]>,
}

impl CodePointSet {
    fn of(set: CodePointSetDataBorrowed<'static>) -> Self {
        const PLANE_ONE: u32 = 0x1_0000;
        Self {
            basic: Coverage::from_codepoints(
                set.iter_ranges()
                    .flat_map(|range| *range.start()..=(*range.end()).min(PLANE_ONE - 1)),
            ),
            astral: set
                .iter_ranges()
                .filter(|range| *range.end() >= PLANE_ONE)
                .map(|range| (*range.start()).max(PLANE_ONE)..=*range.end())
                .collect(),
        }
    }

    fn contains(&self, ch: char) -> bool {
        if ch <= '\u{FFFF}' {
            return self.basic.contains(ch);
        }
        let codepoint = u32::from(ch);
        self.astral
            .binary_search_by(|range| {
                if *range.end() < codepoint {
                    Ordering::Less
                } else if *range.start() > codepoint {
                    Ordering::Greater
                } else {
                    Ordering::Equal
                }
            })
            .is_ok()
    }
}
