use icu_properties::{
    CodePointMapData, PropertyNamesShortBorrowed,
    props::{BidiMirroringGlyph, BidiPairedBracketType, Script as UnicodeScript},
    script::ScriptWithExtensions,
};
use smallvec::SmallVec;

/// A writing system, as its ISO 15924 code such as `Latn` or `Deva`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Script([u8; 4]);

impl Script {
    /// Text that belongs to no one script, such as digits and punctuation
    /// alone: `Zyyy`.
    pub const COMMON: Self = Self(*b"Zyyy");
    /// The Latin script: `Latn`.
    pub const LATIN: Self = Self(*b"Latn");

    /// The ISO 15924 code.
    pub fn tag(self) -> [u8; 4] {
        self.0
    }

    // Whether letter spacing spreads this script's letters apart. Android
    // leaves out the scripts whose letters join, cursively or along a
    // headline.
    pub(crate) fn takes_letter_spacing(self) -> bool {
        !matches!(
            &self.0,
            b"Arab"
                | b"Nkoo"
                | b"Phlp"
                | b"Mand"
                | b"Mong"
                | b"Phag"
                | b"Deva"
                | b"Beng"
                | b"Guru"
                | b"Modi"
                | b"Shrd"
                | b"Sylo"
                | b"Tirh"
                | b"Ogam"
        )
    }

    fn of(script: UnicodeScript) -> Self {
        PropertyNamesShortBorrowed::<UnicodeScript>::new()
            .get(script)
            .and_then(|name| name.as_bytes().try_into().ok())
            .map_or(Self::COMMON, Self)
    }
}

// The script of ASCII `text`: Latin when it holds a letter, else Common.
pub(crate) fn ascii_script(text: &str) -> Script {
    if text.bytes().any(|byte| byte.is_ascii_alphabetic()) {
        Script::LATIN
    } else {
        Script::COMMON
    }
}

// Pushes the script runs of `text` onto `runs`, each as the end of its byte
// range offset by `offset` and its script, by the rules of UAX #24: common and
// inherited characters (spaces, digits, punctuation, combining marks) join
// the run around them, a closing bracket takes the script of its opening
// bracket, and a character whose script extensions hold the run's script
// stays in the run.
pub(crate) fn script_runs(text: &str, offset: usize, runs: &mut Vec<(usize, Script)>) {
    if text.is_ascii() {
        runs.push((offset + text.len(), ascii_script(text)));
        return;
    }
    let scripts = ScriptWithExtensions::new();
    let mirroring = CodePointMapData::<BidiMirroringGlyph>::new();
    let mut run = UnicodeScript::Common;
    // Each open bracket's closing bracket and the script at the opening.
    let mut open: SmallVec<[(char, UnicodeScript); 8]> = SmallVec::new();
    for (at, ch) in text.char_indices() {
        let mut script = scripts.get_script_val(ch);
        if matches!(script, UnicodeScript::Common | UnicodeScript::Inherited) {
            let bracket = mirroring.get(ch);
            match bracket.paired_bracket_type {
                BidiPairedBracketType::Open => {
                    open.push((bracket.mirroring_glyph.unwrap_or(ch), run));
                }
                BidiPairedBracketType::Close => {
                    if let Some(at) = open.iter().rposition(|(closing, _)| *closing == ch) {
                        script = open[at].1;
                        open.truncate(at);
                    }
                }
                _ => {}
            }
        }
        if matches!(
            script,
            UnicodeScript::Common | UnicodeScript::Inherited | UnicodeScript::Unknown
        ) || script == run
            || (run != UnicodeScript::Common && scripts.has_script(ch, run))
        {
            continue;
        }
        if run == UnicodeScript::Common {
            for (_, opening) in &mut open {
                if *opening == UnicodeScript::Common {
                    *opening = script;
                }
            }
        } else {
            runs.push((offset + at, Script::of(run)));
        }
        run = script;
    }
    runs.push((offset + text.len(), Script::of(run)));
}
