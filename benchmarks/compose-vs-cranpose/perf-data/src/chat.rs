//! The chat beside the patchwork: an agent's answers arrive from a data
//! thread as tokens, several a frame, and the screen shows them as they come.
//! One turn holds a question, progress lines, paragraphs and a code block
//! that appear a few characters a token, a table that appears a row a token,
//! an animated picture, a page in a web view and a row of buttons; the turns
//! follow one another forever.
//!
//! The turn is a constant and a token is two numbers, so the data layer
//! costs nothing to speak of: what the chat measures is the way from the
//! thread to the picture. `../README.md` describes the blocks.

use std::{ops::Range, sync::OnceLock};

/// The chat's width at the right of the screen, in points.
pub const CHAT_WIDTH: f32 = 320.0;
/// The tokens the screen takes from the data thread each frame.
pub const CHAT_TOKENS_PER_FRAME: usize = 4;
/// The characters of a paragraph or of a code block that one token adds.
pub const CHAT_TOKEN_CHARS: usize = 6;
/// The tokens a progress line stays live for.
pub const CHAT_PROGRESS_TOKENS: usize = 24;
/// The columns of the chat's table.
pub const CHAT_TABLE_COLUMNS: usize = 4;
/// The frames each frame of the animated picture shows for.
pub const CHAT_GIF_HOLD: u32 = 2;
/// The degrees the live progress line's spinner turns each frame.
pub const CHAT_SPIN_DEGREES: f32 = 12.0;
/// The points the live progress line's gradient moves each frame, and the
/// length of one pass of its colors.
pub const CHAT_SHIMMER_STEP: f32 = 6.0;
pub const CHAT_SHIMMER_SPAN: f32 = 120.0;
/// The width and height of the web view, in points.
pub const CHAT_WEB_SIZE: (f32, f32) = (288.0, 104.0);
/// The side of the animated picture, in points.
pub const CHAT_GIF_SIZE: f32 = 64.0;
/// The size of the chat's text and of its code, in points.
pub const CHAT_TEXT: f32 = 12.0;
pub const CHAT_CODE_TEXT: f32 = 11.0;

/// The chat's background, the fill of a question, the fill of a code block
/// and of a table's header, and the two colors of the live progress line.
pub const CHAT_BACKGROUND_RGB: [u8; 3] = [0x14, 0x18, 0x2B];
pub const CHAT_USER_RGB: [u8; 3] = [0x2B, 0x3A, 0x67];
pub const CHAT_PANEL_RGB: [u8; 3] = [0x0B, 0x0E, 0x1A];
pub const CHAT_SHIMMER_RGB: [[u8; 3]; 2] = [[0x64, 0x74, 0x8B], [0xFF, 0xFF, 0xFF]];

/// The animated picture, a GIF of 16 frames (`../chat-assets/make_gif.py`).
pub static CHAT_GIF: &[u8] = include_bytes!("../../chat-assets/spin.gif");
/// The spinner of a progress line.
pub static CHAT_SVG: &str = include_str!("../../chat-assets/progress.svg");
/// The page the web view shows.
pub static CHAT_HTML: &str = include_str!("../../chat-assets/card.html");

/// The page as a `data:` address a web view loads without a file or a
/// server, built once.
pub fn chat_html_url() -> &'static str {
    static URL: OnceLock<String> = OnceLock::new();
    URL.get_or_init(|| {
        let mut url = String::from("data:text/html;charset=utf-8,");
        for byte in CHAT_HTML.bytes() {
            if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
                url.push(char::from(byte));
            } else {
                url.push_str(&format!("%{byte:02X}"));
            }
        }
        url
    })
}

/// One block of a turn.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ChatBlock {
    /// The user's question, whole at once in a bubble at the right.
    User(&'static str),
    /// A spinner and a label under a moving gradient while the block is the
    /// last one; afterwards the spinner rests and the label is plain.
    Progress(&'static str),
    /// Text that appears `CHAT_TOKEN_CHARS` characters a token.
    Paragraph(&'static str),
    /// Code in a monospaced face with its keywords, strings, numbers and
    /// comments colored, which appears like a paragraph.
    Code(&'static str),
    /// A table whose first row is its header; a row appears a token.
    Table(&'static [[&'static str; CHAT_TABLE_COLUMNS]]),
    /// The animated picture.
    Gif,
    /// The web view.
    Web,
    /// A row of buttons.
    Buttons(&'static [&'static str]),
}

/// One turn of the chat.
pub static CHAT_TURN: [ChatBlock; 11] = [
    ChatBlock::User("Why does the list stutter when one row changes?"),
    ChatBlock::Progress("Reading the layout pass"),
    ChatBlock::Paragraph(
        "The row measures its text again on every frame. Its parent then lays out all of \
         its children, although only one of them changed size. A boundary around the row \
         keeps the work inside it.",
    ),
    ChatBlock::Code(
        "// Measure only the row that changed.\n\
         fn relayout(rows: &mut [Row], dirty: usize) -> f32 {\n\
         \x20   let row = &mut rows[dirty];\n\
         \x20   if row.width == 320.0 {\n\
         \x20       return row.height;\n\
         \x20   }\n\
         \x20   row.label = \"measured\";\n\
         \x20   row.height = row.measure(320.0);\n\
         \x20   row.height\n\
         }\n",
    ),
    ChatBlock::Paragraph(
        "With the boundary the frame lays out one row, not the list. These are the passes \
         before and after the change:",
    ),
    ChatBlock::Table(&[
        ["Pass", "Before", "After", "Change"],
        ["Compose", "1.92 ms", "0.31 ms", "-84%"],
        ["Measure", "4.70 ms", "0.22 ms", "-95%"],
        ["Place", "1.18 ms", "0.09 ms", "-92%"],
        ["Draw", "2.46 ms", "2.41 ms", "-2%"],
        ["Frame", "10.26 ms", "3.03 ms", "-70%"],
    ]),
    ChatBlock::Gif,
    ChatBlock::Progress("Running the benchmark"),
    ChatBlock::Web,
    ChatBlock::Paragraph("The report above holds the numbers of the last run."),
    ChatBlock::Buttons(&["Apply", "Show diff", "Dismiss"]),
];

impl ChatBlock {
    /// The tokens the block takes to appear whole.
    pub fn tokens(self) -> usize {
        match self {
            Self::User(_) | Self::Gif | Self::Web | Self::Buttons(_) => 1,
            Self::Progress(_) => CHAT_PROGRESS_TOKENS,
            Self::Paragraph(text) | Self::Code(text) => text.len().div_ceil(CHAT_TOKEN_CHARS),
            Self::Table(rows) => rows.len(),
        }
    }
}

/// What the data thread sends: the chat's last block and how much of it
/// shows. Every block before it shows whole.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ChatToken {
    /// The index of the last block since the chat began: block `block` is
    /// `CHAT_TURN[block % CHAT_TURN.len()]` of turn `block / CHAT_TURN.len()`.
    pub block: usize,
    /// The tokens of that block that arrived: `CHAT_TOKEN_CHARS` characters
    /// each of a paragraph or of code, a row each of a table.
    pub shown: usize,
}

impl ChatToken {
    /// The chat after `count` tokens, `count` at least 1.
    pub fn after(count: usize) -> Self {
        static STARTS: OnceLock<([usize; CHAT_TURN.len()], usize)> = OnceLock::new();
        let (starts, turn_tokens) = STARTS.get_or_init(|| {
            let mut starts = [0; CHAT_TURN.len()];
            let mut total = 0;
            for (start, block) in starts.iter_mut().zip(CHAT_TURN) {
                *start = total;
                total += block.tokens();
            }
            (starts, total)
        });
        let index = count.max(1) - 1;
        let (turn, within) = (index / turn_tokens, index % turn_tokens);
        let block = starts.partition_point(|start| *start <= within) - 1;
        Self {
            block: turn * CHAT_TURN.len() + block,
            shown: within - starts[block] + 1,
        }
    }

    /// The turn of block `block`.
    pub fn turn(block: usize) -> usize {
        block / CHAT_TURN.len()
    }
}

/// The characters of `text` that `shown` tokens show.
pub fn chat_shown_chars(text: &str, shown: usize) -> usize {
    (shown * CHAT_TOKEN_CHARS).min(text.len())
}

/// The line above a paragraph or a code block that names its turn, so that
/// no turn shows the text of another: `turn` in five digits.
pub fn chat_turn_label(turn: usize) -> String {
    format!("#{:05}", turn % 100_000)
}

/// What a stretch of code is, for its color.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CodeKind {
    Keyword,
    Text,
    Number,
    Comment,
}

impl CodeKind {
    /// The color of code of this kind; other code is white.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Self::Keyword => [0xC0, 0x84, 0xFC],
            Self::Text => [0x86, 0xEF, 0xAC],
            Self::Number => [0xFD, 0xBA, 0x74],
            Self::Comment => [0x64, 0x74, 0x8B],
        }
    }
}

const KEYWORDS: [&str; 8] = ["fn", "let", "mut", "if", "return", "usize", "f32", "Row"];

/// The colored stretches of `code`, in order, as byte ranges: its keywords,
/// its quoted text, its numbers and its `//` comments. Code is ASCII, so a
/// byte is a character.
pub fn chat_code_spans(code: &str) -> Vec<(Range<usize>, CodeKind)> {
    let bytes = code.as_bytes();
    let mut spans = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let start = at;
        let byte = bytes[at];
        if byte == b'/' && bytes.get(at + 1) == Some(&b'/') {
            at = code[at..].find('\n').map_or(bytes.len(), |end| at + end);
            spans.push((start..at, CodeKind::Comment));
        } else if byte == b'"' {
            at = code[at + 1..]
                .find('"')
                .map_or(bytes.len(), |end| at + end + 2);
            spans.push((start..at, CodeKind::Text));
        } else if byte.is_ascii_alphabetic() || byte == b'_' {
            while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
                at += 1;
            }
            if KEYWORDS.contains(&&code[start..at]) {
                spans.push((start..at, CodeKind::Keyword));
            }
        } else if byte.is_ascii_digit() {
            while at < bytes.len() && (bytes[at].is_ascii_digit() || bytes[at] == b'.') {
                at += 1;
            }
            spans.push((start..at, CodeKind::Number));
        } else {
            at += 1;
        }
    }
    spans
}

/// The frame of the animated picture that frame `frame` of the screen shows,
/// of `frames` frames.
pub fn chat_gif_frame(frame: u32, frames: usize) -> usize {
    (frame / CHAT_GIF_HOLD) as usize % frames.max(1)
}

/// The tokens of the chat as they leave the data layer. On a platform with
/// threads a thread of its own sends them through a bounded channel and
/// waits while the screen is behind; a browser page has no threads, so
/// there the feed counts the tokens as the screen takes them.
pub struct ChatFeed {
    #[cfg(not(target_arch = "wasm32"))]
    tokens: std::sync::mpsc::Receiver<ChatToken>,
    #[cfg(target_arch = "wasm32")]
    sent: std::cell::Cell<usize>,
}

impl ChatFeed {
    /// Starts the data layer.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn start() -> std::io::Result<Self> {
        let (sender, tokens) = std::sync::mpsc::sync_channel(CHAT_TOKENS_PER_FRAME * 4);
        std::thread::Builder::new()
            .name("chat-data".into())
            .spawn(move || {
                let mut count = 0;
                loop {
                    count += 1;
                    // The screen is gone once it dropped its end.
                    if sender.send(ChatToken::after(count)).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self { tokens })
    }

    /// Starts the data layer.
    #[cfg(target_arch = "wasm32")]
    pub fn start() -> std::io::Result<Self> {
        Ok(Self {
            sent: std::cell::Cell::new(0),
        })
    }

    /// The next token, when one has arrived. The screen never waits for one.
    pub fn take(&self) -> Option<ChatToken> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.tokens.try_recv().ok()
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.sent.set(self.sent.get() + 1);
            Some(ChatToken::after(self.sent.get()))
        }
    }
}
