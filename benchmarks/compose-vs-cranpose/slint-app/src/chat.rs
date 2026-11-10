//! The chat beside the patchwork (`../README.md`): the model behind the
//! blocks of `ui/chat.slint`, written to as tokens arrive.
//!
//! Each frame takes up to four tokens from the data thread and writes the last
//! block once. A paragraph replaces its row's text. Code and a table keep one
//! model for the whole block and add a line or a row to it, so a token changes
//! nothing but the line or the row it reaches. The model keeps the blocks of
//! the last two turns.

use std::{cell::Cell, fmt::Write, ops::Range, rc::Rc, time::Duration};

use perf_data::{
    CHAT_GIF, CHAT_SVG, CHAT_TABLE_COLUMNS, CHAT_TOKENS_PER_FRAME, CHAT_TURN, ChatBlock, ChatFeed,
    ChatToken, CodeKind, chat_code_spans, chat_gif_frame, chat_shown_chars, chat_status,
    chat_turn_label,
};
use slint::{
    Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, StyledText, VecModel,
};

use super::{BlockKind, ChatItem, CodeLine, TableRow};

/// The turns the model keeps.
const KEPT_TURNS: usize = 2;

/// The chat's model rows and the images its blocks show.
pub struct ChatModel {
    feed: Option<ChatFeed>,
    items: Rc<VecModel<ChatItem>>,
    status: Rc<VecModel<SharedString>>,
    /// The block of the model's first row, and the first block that stays once
    /// the queued removals have run.
    first: Rc<Cell<usize>>,
    keep_from: usize,
    /// The last token taken.
    live: Option<ChatToken>,
    /// Whether the last block lacks tokens already taken.
    pending: bool,
    /// How the last block takes tokens.
    grow: Grow,
    gif: Vec<Image>,
    gif_shown: Option<usize>,
}

impl ChatModel {
    pub fn new() -> Self {
        let feed = ChatFeed::start()
            .map_err(|error| log::warn!("chat data thread: {error}"))
            .ok();
        Self {
            feed,
            items: Rc::new(VecModel::default()),
            status: Rc::new(VecModel::default()),
            first: Rc::new(Cell::new(0)),
            keep_from: 0,
            live: None,
            pending: false,
            grow: Grow::Text,
            gif: decode_gif(),
            gif_shown: None,
        }
    }

    pub fn items(&self) -> ModelRc<ChatItem> {
        self.items.clone().into()
    }

    pub fn status(&self) -> ModelRc<SharedString> {
        self.status.clone().into()
    }

    /// The status line's spinner.
    pub fn spinner() -> Image {
        Image::load_from_svg_data(CHAT_SVG.as_bytes()).unwrap_or_else(|error| {
            log::warn!("chat spinner: {error}");
            Image::default()
        })
    }

    /// Takes the frame's tokens and writes the rows they changed. Returns the
    /// animated picture's image when frame `frame` shows another one.
    pub fn advance(&mut self, frame: u32) -> Option<Image> {
        let mut token = None;
        for _ in 0..CHAT_TOKENS_PER_FRAME {
            let Some(next) = self.feed.as_ref().and_then(ChatFeed::take) else {
                break;
            };
            self.take(next);
            token = Some(next);
        }
        if let (Some(live), true) = (self.live, self.pending) {
            self.write(live.block, live.shown);
            self.pending = false;
        }
        if let Some(token) = token {
            sync_glyphs(&self.status, &chat_status(token));
        }
        let index = chat_gif_frame(frame, self.gif.len());
        let shown = self.gif_shown.replace(index);
        (shown != Some(index))
            .then(|| self.gif.get(index).cloned())
            .flatten()
    }

    // A token of the block already shown only notes how much of it shows. A
    // token of the next block writes the one before whole and starts a row.
    fn take(&mut self, token: ChatToken) {
        match self.live {
            Some(live) if live.block == token.block => self.pending = true,
            previous => {
                if let (Some(previous), true) = (previous, self.pending) {
                    self.write(previous.block, usize::MAX);
                }
                let (item, grow) = start(token.block, token.shown);
                self.items.push(item);
                self.grow = grow;
                self.pending = false;
                self.trim(ChatToken::turn(token.block));
            }
        }
        self.live = Some(token);
    }

    // Shows `shown` tokens of block `block`, the last one.
    fn write(&mut self, block: usize, shown: usize) {
        match &self.grow {
            Grow::Whole => {}
            Grow::Text => self
                .items
                .set_row_data(block - self.first.get(), retext(block, shown)),
            Grow::Code(code) => code.show(chat_shown_chars(code.code, shown)),
            Grow::Table(table) => table.show(shown),
        }
    }

    // Drops the blocks of turns before the last `KEPT_TURNS`. Slint frees the
    // elements of a removed row through the renderer, which is busy while the
    // frame loop runs, so the removal runs on the event loop.
    fn trim(&mut self, turn: usize) {
        let keep_from = (turn + 1).saturating_sub(KEPT_TURNS) * CHAT_TURN.len();
        let count = keep_from.saturating_sub(self.keep_from);
        if count == 0 {
            return;
        }
        self.keep_from = keep_from;
        let (items, first) = (self.items.clone(), self.first.clone());
        slint::Timer::single_shot(Duration::ZERO, move || {
            for _ in 0..count {
                items.remove(0);
            }
            first.set(first.get() + count);
        });
    }
}

// The glyph rows show `text`; those that differ are replaced, the others
// stay. A shorter text blanks the rows it leaves: removing a row waits for the
// event loop, and the rows are at most as many as the longest status.
fn sync_glyphs(model: &VecModel<SharedString>, text: &str) {
    let mut glyphs = text.chars().map(SharedString::from);
    for at in 0..model.row_count().max(text.chars().count()) {
        let glyph = glyphs.next().unwrap_or_default();
        if at >= model.row_count() {
            model.push(glyph);
        } else if model.row_data(at).is_none_or(|shown| shown != glyph) {
            model.set_row_data(at, glyph);
        }
    }
}

/// How the last block takes tokens: a paragraph's text is replaced, code and
/// a table add to the model their row keeps.
enum Grow {
    /// A block whole from its first token.
    Whole,
    Text,
    Code(LiveCode),
    Table(LiveTable),
}

// The first row of block `block` with `shown` tokens of it, and how its next
// tokens reach it.
fn start(block: usize, shown: usize) -> (ChatItem, Grow) {
    let mut item = retext(block, shown);
    let grow = match CHAT_TURN[block % CHAT_TURN.len()] {
        ChatBlock::Heading(_) | ChatBlock::Paragraph(_) => Grow::Text,
        ChatBlock::User(_) => Grow::Whole,
        ChatBlock::Code(code) => {
            let live = LiveCode::new(ChatToken::turn(block), code);
            live.show(chat_shown_chars(code, shown));
            item.kind = BlockKind::Code;
            item.lines = live.model.clone().into();
            Grow::Code(live)
        }
        ChatBlock::Table(rows) => {
            let live = LiveTable {
                rows,
                model: Rc::new(VecModel::default()),
            };
            live.show(shown);
            item.kind = BlockKind::Table;
            item.rows = live.model.clone().into();
            Grow::Table(live)
        }
        ChatBlock::Gif => {
            item.kind = BlockKind::Gif;
            Grow::Whole
        }
        ChatBlock::Buttons(labels) => {
            item.kind = BlockKind::Buttons;
            item.buttons = ModelRc::new(VecModel::from(
                labels
                    .iter()
                    .map(|label| (*label).into())
                    .collect::<Vec<_>>(),
            ));
            Grow::Whole
        }
    };
    (item, grow)
}

// The row of a text block with `shown` tokens of it shown.
fn retext(block: usize, shown: usize) -> ChatItem {
    let turn = ChatToken::turn(block);
    let typed = |text: &str| {
        format!(
            "{} {}",
            chat_turn_label(turn),
            &text[..chat_shown_chars(text, shown)]
        )
    };
    let (kind, text) = match CHAT_TURN[block % CHAT_TURN.len()] {
        ChatBlock::User(text) => (BlockKind::User, text.to_string()),
        ChatBlock::Heading(text) => (BlockKind::Heading, typed(text)),
        ChatBlock::Paragraph(text) => (BlockKind::Paragraph, typed(text)),
        _ => (BlockKind::Paragraph, String::new()),
    };
    ChatItem {
        kind,
        text: text.into(),
        lines: ModelRc::default(),
        rows: ModelRc::default(),
        buttons: ModelRc::default(),
    }
}

/// A code block's lines, made whole once, and the model that shows the lines
/// reached so far.
struct LiveCode {
    code: &'static str,
    turn: usize,
    lines: Vec<CodeLineSource>,
    model: Rc<VecModel<CodeLine>>,
}

/// One line of code: where it starts and its colored stretches, as byte
/// ranges of the code (a stretch of no kind is white).
struct CodeLineSource {
    start: usize,
    stretches: Vec<(Range<usize>, Option<CodeKind>)>,
}

impl LiveCode {
    fn new(turn: usize, code: &'static str) -> Self {
        let mut lines = vec![CodeLineSource {
            start: 0,
            stretches: Vec::new(),
        }];
        let mut add = |range: Range<usize>, kind: Option<CodeKind>| {
            let mut at = range.start;
            for (number, text) in code[range].split('\n').enumerate() {
                if number > 0 {
                    lines.push(CodeLineSource {
                        start: at,
                        stretches: Vec::new(),
                    });
                }
                if !text.is_empty() {
                    if let Some(line) = lines.last_mut() {
                        line.stretches.push((at..at + text.len(), kind));
                    }
                }
                at += text.len() + 1;
            }
        };
        let mut end = 0;
        for (span, kind) in chat_code_spans(code) {
            add(end..span.start, None);
            end = span.end;
            add(span, Some(kind));
        }
        add(end..code.len(), None);
        Self {
            code,
            turn,
            lines,
            model: Rc::new(VecModel::default()),
        }
    }

    // The row `row` of the block with the first `chars` characters shown: the
    // comment that names the turn, then the lines of code. A line is markdown
    // of `<font>` stretches, which Slint wraps at the panel's width.
    fn row(&self, row: usize, chars: usize) -> CodeLine {
        let mut markdown = String::new();
        match row.checked_sub(1) {
            None => stretch(
                &mut markdown,
                &format!("// {}", chat_turn_label(self.turn)),
                Some(CodeKind::Comment),
            ),
            Some(line) => {
                for (range, kind) in &self.lines[line].stretches {
                    if range.start < chars {
                        let text = &self.code[range.start..range.end.min(chars)];
                        stretch(&mut markdown, text, *kind);
                    }
                }
            }
        }
        if markdown.is_empty() {
            markdown.push(NO_BREAK_SPACE);
        }
        CodeLine {
            text: StyledText::from_markdown(&markdown).unwrap_or_else(|error| {
                log::warn!("chat code: {error}");
                StyledText::default()
            }),
        }
    }

    // Shows the first `chars` characters: the line the last token ended in is
    // written again, the lines it began are added.
    fn show(&self, chars: usize) {
        let needed = 1 + self.lines.iter().filter(|line| line.start <= chars).count();
        for row in self.model.row_count().saturating_sub(1)..needed {
            if row < self.model.row_count() {
                self.model.set_row_data(row, self.row(row, chars));
            } else {
                self.model.push(self.row(row, chars));
            }
        }
    }
}

/// Markdown drops the spaces that begin a line and joins the others: the
/// indentation is made of this character.
const NO_BREAK_SPACE: char = '\u{a0}';

// Appends `text` of `kind` to a line of markdown: every punctuation mark is
// escaped, the spaces that begin the line are no-break spaces, and a colored
// kind sits in a `<font>` tag.
fn stretch(line: &mut String, text: &str, kind: Option<CodeKind>) {
    if let Some(kind) = kind {
        let [red, green, blue] = kind.rgb();
        let _ = write!(line, "<font color=\"#{red:02X}{green:02X}{blue:02X}\">");
    }
    let mut indent = line.is_empty() || line.chars().all(|c| c == NO_BREAK_SPACE);
    for character in text.chars() {
        indent &= character == ' ';
        if indent {
            line.push(NO_BREAK_SPACE);
            continue;
        }
        if character.is_ascii_punctuation() {
            line.push('\\');
        }
        line.push(character);
    }
    if kind.is_some() {
        line.push_str("</font>");
    }
}

/// A table's rows, and the model that shows those that arrived.
struct LiveTable {
    rows: &'static [[&'static str; CHAT_TABLE_COLUMNS]],
    model: Rc<VecModel<TableRow>>,
}

impl LiveTable {
    // Adds the rows `shown` tokens bring.
    fn show(&self, shown: usize) {
        for at in self.model.row_count()..shown.min(self.rows.len()) {
            self.model.push(TableRow {
                cells: ModelRc::new(VecModel::from(
                    self.rows[at]
                        .iter()
                        .map(|cell| (*cell).into())
                        .collect::<Vec<_>>(),
                )),
                header: at == 0,
                shaded: at % 2 == 1,
            });
        }
    }
}

// Every frame of the animated picture; none when it does not decode.
fn decode_gif() -> Vec<Image> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut frames = Vec::new();
    match options.read_info(CHAT_GIF) {
        Ok(mut decoder) => {
            let (width, height) = (u32::from(decoder.width()), u32::from(decoder.height()));
            while let Ok(Some(frame)) = decoder.read_next_frame() {
                frames.push(Image::from_rgba8(
                    SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&frame.buffer, width, height),
                ));
            }
        }
        Err(error) => log::warn!("chat picture: {error}"),
    }
    frames
}
