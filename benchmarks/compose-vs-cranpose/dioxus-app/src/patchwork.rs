//! The patchwork in Dioxus (`../README.md`): nine patches of the still
//! wall's tiles beside a chat. Two patches never change; in the others a
//! frame changes a small part.
//!
//! In Dioxus's idiom a component renders again when a signal it read
//! changes. A row of tiles reads a memo of how often it changed, so a frame
//! renders the one row whose turn it is; a card's body and a block of the
//! chat that is whole read no signal and render once. The components that
//! read the frame are small: a card's box, which sets its transform, the
//! reorder patch's list of keyed items, the status line. The page's CSS
//! (`web-app/www/style.css`) lays the patches out, as the web page does.

use dioxus::prelude::*;
use perf_data::*;

use super::{ASSETS, FRAME, hex, platform, use_frames, wall::Clock};

/// Marks the page's body as the wall's, which its CSS colours.
const STILL_WALL: &str = "document.body.classList.add('still-wall')";
// The turns whose blocks the chat's list holds: more than it shows.
const TURNS_HELD: usize = 2;

/// The chat's last token, which the frame loop writes as tokens arrive.
static TOKEN: GlobalSignal<Option<ChatToken>> = Signal::global(|| None);

thread_local! {
    // The chat's data thread, started with the first frame.
    static FEED: Option<ChatFeed> = ChatFeed::start()
        .map_err(|error| log::warn!("chat data thread: {error}"))
        .ok();
}

// Takes the frame's tokens from the data thread.
fn take_tokens(_: u32) {
    let last = FEED.with(|feed| {
        let feed = feed.as_ref()?;
        (0..CHAT_TOKENS_PER_FRAME).map_while(|_| feed.take()).last()
    });
    if last.is_some() {
        *TOKEN.write() = last;
    }
}

type Grid = PatchTiles;

#[component]
pub(crate) fn PatchworkApp() -> Element {
    use_frames(take_tokens);
    use_hook(|| document::eval(STILL_WALL));
    let tier = wall_tier(perf_data::launch().tier);
    let (width, height) = platform::viewport();
    let patch_width = ((width as f32 - CHAT_WIDTH) / PATCH_COLUMNS as f32).floor();
    let patch_height = ((height as f32 - 56.0 - WALL_CLOCK_HEIGHT) / PATCH_ROWS as f32).floor();
    let grid = Grid::new(tier, patch_width, patch_height);
    let columns = grid.columns;
    let cards = Cards::new(grid.columns, grid.rows, grid.height, patch_height);
    let (label, value) = wall_text_size(grid.height);
    let style = format!(
        "--width: {}px; --height: {}px; --label: {label}px; --value: {value}px; --dot: {}px; \
         --pad: {}px; --columns: {columns}; --patch-width: {patch_width}px; \
         --patch-height: {patch_height}px",
        grid.width,
        grid.height,
        wall_dot_size(grid.height),
        wall_padding(grid.height),
    );
    rsx! {
        header { class: "top-bar", "Patchwork" }
        Clock {}
        div { class: "patchwork-screen",
            div { class: "patchwork", style: "{style}",
                Still { tiles: grid.count() }
                Charts { grid }
                Recolor { grid }
                CardStack { cards }
                Reorder { grid }
                Deep { grid }
                Scroll { grid }
                Paragraph { grid }
                Pile { grid }
            }
            Chat {}
        }
    }
}

// Tile `index` on `fill`, with `style` for its place or its size.
fn tile(index: usize, fill: String, style: String) -> Element {
    rsx! {
        div {
            key: "{index}",
            class: "wall-tile",
            background: "{fill}",
            style: "{style}",
            div { class: "wall-label", "{wall_label(index)}" }
            div { class: "wall-row",
                div { class: "wall-dot", background: "{hex(&wall_dot_rgb(index))}" }
                div { class: "wall-value", "{wall_value(index)}" }
            }
        }
    }
}

fn wall_tile(index: usize) -> Element {
    tile(index, hex(&wall_fill_rgb(index)), String::new())
}

/// Tiles that read no signal.
#[component]
fn Still(tiles: usize) -> Element {
    rsx! {
        div { class: "patch patch-grid",
            for index in 0..tiles {
                {wall_tile(index)}
            }
        }
    }
}

/// Line charts that read no signal, each an SVG line.
#[component]
fn Charts(grid: Grid) -> Element {
    let (columns, rows) = patchwork_pairs(grid.columns, grid.rows);
    let (width, height) = (patchwork_double(grid.width), patchwork_double(grid.height));
    let step = width / (PATCHWORK_CHART_POINTS - 1) as f32;
    let points = |index: usize| {
        (0..PATCHWORK_CHART_POINTS)
            .map(|point| {
                format!(
                    "{},{}",
                    point as f32 * step,
                    height * (1.0 - patchwork_chart_value(index, point))
                )
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    rsx! {
        div { class: "patch patch-grid patch-pairs", style: "--pairs: {columns}",
            for index in 0..columns * rows {
                svg { key: "{index}", class: "patch-chart",
                    polyline {
                        points: "{points(index)}",
                        stroke: "{hex(&patchwork_chart_rgb(index))}",
                    }
                }
            }
        }
    }
}

#[component]
fn Recolor(grid: Grid) -> Element {
    rsx! {
        div { class: "patch patch-grid",
            for row in 0..grid.rows {
                RecolorRow { key: "{row}", first: row * grid.columns, grid }
            }
        }
    }
}

/// A row of tiles whose fill changes in turn: it reads how often the row
/// changed, so it renders when its turn comes.
#[component]
fn RecolorRow(first: usize, grid: Grid) -> Element {
    let turn = grid.in_turn();
    let step = use_memo(move || turn.step(first, FRAME()));
    let frame = *FRAME.peek();
    step();
    rsx! {
        for index in first..first + grid.columns {
            {tile(index, hex(&patchwork_recolor_rgb(index, frame, turn)), String::new())}
        }
    }
}

/// The cards of the move that runs: it reads the number of moves, so it
/// renders when a card comes in or the last one has left.
#[component]
fn CardStack(cards: Cards) -> Element {
    let moves = use_memo(|| FRAME() / PATCHWORK_CARD_FRAMES);
    let begun = moves() * PATCHWORK_CARD_FRAMES;
    rsx! {
        div { class: "patch patch-cards",
            for card in (0..cards.places()).filter_map(|place| cards.place(place, begun)) {
                Card { key: "{card.id}", id: card.id, cards }
            }
        }
    }
}

/// A card's box: it reads the frame and sets the card's place.
#[component]
fn Card(id: usize, cards: Cards) -> Element {
    let top = cards.top(id, FRAME()).unwrap_or(-cards.height);
    rsx! {
        div { class: "patch-card", style: "transform: translateY({top}px)",
            CardBody { id, cards }
        }
    }
}

/// A card's header and tiles, which read no signal.
#[component]
fn CardBody(id: usize, cards: Cards) -> Element {
    rsx! {
        div {
            class: "patch-card-title",
            background: "{hex(&patchwork_card_rgb(id))}",
            "{patchwork_card_title(id)}"
        }
        div { class: "patch-grid",
            for place in 0..cards.tiles() {
                {wall_tile(patchwork_card_tile(id, place, cards.tiles()))}
            }
        }
    }
}

/// The wrapping row's items in the frame's order, under their keys.
#[component]
fn Reorder(grid: Grid) -> Element {
    let count = patchwork_reorder_count(grid.count());
    let frame = FRAME();
    rsx! {
        div { class: "patch patch-flow",
            for item in (0..count).map(|position| patchwork_reorder_item(position, frame, count)) {
                ReorderItem { key: "{item}", item, width: grid.width }
            }
        }
    }
}

#[component]
fn ReorderItem(item: usize, width: f32) -> Element {
    let width = patchwork_reorder_width(patchwork_reorder_units(item), width);
    tile(item, hex(&wall_fill_rgb(item)), format!("width: {width}px"))
}

#[component]
fn Deep(grid: Grid) -> Element {
    let (columns, rows) = patchwork_pairs(grid.columns, grid.rows);
    rsx! {
        div { class: "patch patch-grid patch-pairs", style: "--pairs: {columns}",
            for row in 0..rows {
                DeepRow { key: "{row}", first: row * columns, columns, rows }
            }
        }
    }
}

// Box `level` of nest `index` and everything inside it, around `value`.
fn nest(index: usize, level: usize, value: &str) -> Element {
    rsx! {
        div {
            class: "patch-nest",
            background: "{hex(&patchwork_deep_rgb(index, level))}",
            if level + 1 < PATCHWORK_DEPTH {
                {nest(index, level + 1, value)}
            } else {
                "{value}"
            }
        }
    }
}

/// A row of nests, which renders when its turn comes.
#[component]
fn DeepRow(first: usize, columns: usize, rows: usize) -> Element {
    let turn = InTurn { columns, rows };
    let step = use_memo(move || turn.step(first, FRAME()));
    let frame = *FRAME.peek();
    step();
    rsx! {
        for index in first..first + columns {
            {nest(index, 0, &patchwork_deep_value(index, frame, turn))}
        }
    }
}

/// The list as a sheet a transform moves; its rows keep their keys, so a
/// frame builds the row that came in and drops the row that left.
#[component]
fn Scroll(grid: Grid) -> Element {
    let total = patchwork_scroll_rows(grid.columns);
    let first = patchwork_scroll_first(FRAME(), total, grid.rows);
    let pitch = grid.height + WALL_GAP;
    rsx! {
        div { class: "patch",
            div {
                class: "patch-sheet",
                style: "height: {total as f32 * pitch}px; will-change: transform; \
                        transform: translateY({-(first as f32) * pitch}px)",
                for row in first..=first + grid.rows {
                    ScrollRow { key: "{row}", row, columns: grid.columns, pitch }
                }
            }
        }
    }
}

#[component]
fn ScrollRow(row: usize, columns: usize, pitch: f32) -> Element {
    rsx! {
        div { class: "patch-row", style: "top: {row as f32 * pitch}px",
            for column in 0..columns {
                {wall_tile(row * columns + column)}
            }
        }
    }
}

#[component]
fn Paragraph(grid: Grid) -> Element {
    let text = patchwork_paragraph(FRAME(), patchwork_paragraph_words(grid.count()));
    rsx! {
        div { class: "patch patch-paragraph", "{text}" }
    }
}

/// The pile reads the frame; a tile renders when it turns opaque or
/// translucent.
#[component]
fn Pile(grid: Grid) -> Element {
    let (columns, rows) = patchwork_pile_grid(grid.columns, grid.rows);
    let opaque = patchwork_pile_opaque(FRAME(), columns * rows);
    rsx! {
        div { class: "patch patch-pile",
            for index in 0..columns * rows {
                PileTile {
                    key: "{index}",
                    index,
                    left: WALL_GAP + (index % columns) as f32 * (grid.width + WALL_GAP),
                    top: WALL_GAP + (index / columns) as f32 * (grid.height + WALL_GAP),
                    opaque: index == opaque,
                }
            }
        }
    }
}

#[component]
fn PileTile(index: usize, left: f32, top: f32, opaque: bool) -> Element {
    let fill = if opaque {
        hex(&PATCHWORK_PILE_OPAQUE_RGB)
    } else {
        format!("{}{PATCHWORK_PILE_ALPHA:02X}", hex(&wall_fill_rgb(index)))
    };
    tile(index, fill, format!("left: {left}px; top: {top}px"))
}

/// The chat: it reads the number of blocks, so it renders when a block
/// begins.
#[component]
fn Chat() -> Element {
    let blocks = use_memo(|| TOKEN().map_or(0, |token| token.block + 1));
    let count = blocks();
    let first = count.saturating_sub(TURNS_HELD * CHAT_TURN.len());
    rsx! {
        div { class: "chat",
            div { class: "chat-scroll",
                div { class: "chat-list",
                    for block in first..count {
                        Block { key: "{block}", block, live: block + 1 == count }
                    }
                }
            }
            Status {}
        }
    }
}

/// Block `block` of the chat. The last one reads the token for how much of
/// it shows; a whole one reads no signal.
#[component]
fn Block(block: usize, live: bool) -> Element {
    let shown = if live {
        TOKEN()
            .filter(|token| token.block == block)
            .map_or(usize::MAX, |token| token.shown)
    } else {
        usize::MAX
    };
    let turn = ChatToken::turn(block);
    let typed = |text: &'static str| &text[..chat_shown_chars(text, shown.min(text.len()))];
    match CHAT_TURN[block % CHAT_TURN.len()] {
        ChatBlock::User(said) => rsx! {
            div { class: "chat-user", "{said}" }
        },
        ChatBlock::Heading(said) => rsx! {
            div { class: "chat-heading", "{chat_turn_label(turn)} {typed(said)}" }
        },
        ChatBlock::Paragraph(said) => rsx! {
            div { class: "chat-paragraph", "{chat_turn_label(turn)} {typed(said)}" }
        },
        ChatBlock::Code(code) => code_block(turn, typed(code)),
        ChatBlock::Table(rows) => rsx! {
            table { class: "chat-table",
                for (index , cells) in rows.iter().take(shown).enumerate() {
                    tr { key: "{index}",
                        for cell in cells {
                            if index == 0 {
                                th { "{cell}" }
                            } else {
                                td { "{cell}" }
                            }
                        }
                    }
                }
            }
        },
        ChatBlock::Gif => rsx! {
            img { class: "chat-gif", src: "{ASSETS}://localhost/chat/spin.gif" }
        },
        ChatBlock::Buttons(labels) => rsx! {
            div { class: "chat-buttons",
                for label in labels {
                    button { "{label}" }
                }
            }
        },
    }
}

// `code` under a comment that names its turn, each stretch in a span of
// its kind's class.
fn code_block(turn: usize, code: &str) -> Element {
    let mut at = 0;
    let mut stretches = Vec::new();
    for (stretch, kind) in chat_code_spans(code) {
        stretches.push(("", &code[at..stretch.start]));
        let class = match kind {
            CodeKind::Keyword => "code-keyword",
            CodeKind::Text => "code-text",
            CodeKind::Number => "code-number",
            CodeKind::Comment => "code-comment",
        };
        stretches.push((class, &code[stretch.clone()]));
        at = stretch.end;
    }
    stretches.push(("", &code[at..]));
    rsx! {
        pre { class: "chat-code",
            span { class: "code-comment", "// {chat_turn_label(turn)}\n" }
            for (class , text) in stretches {
                span { class, "{text}" }
            }
        }
    }
}

/// The status line: it reads the frame and the token, and sets the
/// spinner's turn, the gradient's place and the text.
#[component]
fn Status() -> Element {
    let frame = FRAME();
    let said = TOKEN().map(chat_status).unwrap_or_default();
    let turned = (frame as f32 * CHAT_SPIN_DEGREES) % 360.0;
    let shimmer = (frame as f32 * CHAT_SHIMMER_STEP) % CHAT_SHIMMER_SPAN;
    rsx! {
        div { class: "chat-status",
            div {
                class: "chat-spinner",
                style: "transform: rotate({turned}deg)",
                dangerous_inner_html: CHAT_SVG,
            }
            span { class: "chat-shimmer", style: "background-position-x: {shimmer}px", "{said}" }
        }
    }
}
