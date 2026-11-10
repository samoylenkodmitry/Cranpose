//! The patchwork in Slint (`../README.md`): `ui/patchwork.slint` draws it;
//! this side makes the models once and writes, after every frame drawn, only
//! the rows the next frame changes. The data are the other apps' `perf-data`.

use std::{fmt::Write, rc::Rc, time::Duration};

use perf_data::{
    CHAT_WIDTH, Cards, FrameIndex, InTurn, Launch, PALETTE_RGB, PATCH_COLUMNS, PATCH_ROWS,
    PATCHWORK_CARD_FRAMES, PATCHWORK_CHART_POINTS, PATCHWORK_PILE_ALPHA, PATCHWORK_PILE_OPAQUE_RGB,
    PatchTiles, WALL_CLOCK_HEIGHT, WALL_GAP, WallTier, patchwork_card_rgb, patchwork_card_tile,
    patchwork_card_title, patchwork_chart_rgb, patchwork_chart_value, patchwork_deep_value,
    patchwork_double, patchwork_pairs, patchwork_paragraph, patchwork_paragraph_words,
    patchwork_pile_grid, patchwork_pile_opaque, patchwork_recolor_rgb, patchwork_reorder_count,
    patchwork_reorder_item, patchwork_reorder_swap, patchwork_reorder_units,
    patchwork_reorder_width, patchwork_scroll_first, patchwork_scroll_rows, wall_clock,
    wall_dot_size, wall_fill_rgb, wall_padding, wall_text_size, wall_tier,
};
use slint::{
    Color, ComponentHandle, Model, ModelRc, ModelTracker, PlatformError, RenderingState,
    SharedString, VecModel,
};

use super::{
    ChartData, Metrics, PatchCard, PatchworkWindow, PlacedTile, TileRow, WallClock,
    chat::ChatModel, color, wall_tile,
};

/// The tile row `frame` changes of `rows` rows of an `InTurn`: its step grows
/// for the items of one row each frame.
fn row_in_turn(frame: u32, rows: usize) -> usize {
    (rows - frame as usize % rows) % rows
}

/// The path commands of chart `index` in a `width` by `height` box.
fn chart_path(index: usize, width: f32, height: f32) -> SharedString {
    let mut path = String::with_capacity(PATCHWORK_CHART_POINTS * 16);
    let step = width / (PATCHWORK_CHART_POINTS - 1) as f32;
    for point in 0..PATCHWORK_CHART_POINTS {
        let command = if point == 0 { 'M' } else { 'L' };
        let y = height * (1.0 - patchwork_chart_value(index, point));
        let _ = write!(path, " {command} {:.2} {y:.2}", point as f32 * step);
    }
    path.into()
}

/// The rows of the scroll patch's list, made when the list asks for them.
struct ScrollRows {
    columns: usize,
    rows: usize,
}

impl Model for ScrollRows {
    type Data = TileRow;

    fn row_count(&self) -> usize {
        self.rows
    }

    fn row_data(&self, row: usize) -> Option<TileRow> {
        (row < self.rows).then(|| TileRow {
            tiles: ModelRc::new(VecModel::from(
                (row * self.columns..(row + 1) * self.columns)
                    .map(wall_tile)
                    .collect::<Vec<_>>(),
            )),
        })
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        // The rows never change.
        &()
    }
}

/// The reorder patch: the items in the places of one wrapping row. A place
/// holds an item, its width and where the row put it.
struct Reorder {
    count: usize,
    tile_width: f32,
    tile_height: f32,
    patch_width: f32,
    /// The first of the two places whose items changed places last.
    swap: usize,
    /// What each place holds, to find the places a frame changes.
    held: Vec<Held>,
    placed: Rc<VecModel<PlacedTile>>,
}

#[derive(Clone, Copy, PartialEq)]
struct Held {
    item: usize,
    x: f32,
    y: f32,
    width: f32,
}

impl Reorder {
    fn new(tiles: PatchTiles, patch_width: f32) -> Self {
        let mut reorder = Self {
            count: patchwork_reorder_count(tiles.count()),
            tile_width: tiles.width,
            tile_height: tiles.height,
            patch_width,
            swap: 0,
            held: Vec::new(),
            placed: Rc::new(VecModel::default()),
        };
        reorder.place(0, 0, 0);
        reorder
    }

    // Moves the swap to the places of frame `frame`. The items before the
    // earlier and the later swap stay where they were, and so do those after
    // both as soon as one lies where it did, as the row wraps from the same
    // place on.
    fn advance(&mut self, frame: u32) {
        let swap = patchwork_reorder_swap(frame, self.count);
        let (low, high) = (self.swap.min(swap), self.swap.max(swap));
        self.swap = swap;
        self.place(frame, low, high);
    }

    // Places the items of `frame` from place `low` on in the row and writes
    // the places whose item or position differs from what they held.
    fn place(&mut self, frame: u32, low: usize, high: usize) {
        let (mut x, mut y) = match low.checked_sub(1).map(|before| self.held[before]) {
            Some(before) => (before.x + before.width + WALL_GAP, before.y),
            None => (WALL_GAP, WALL_GAP),
        };
        for position in low..self.count {
            let item = patchwork_reorder_item(position, frame, self.count);
            let width = patchwork_reorder_width(patchwork_reorder_units(item), self.tile_width);
            if x > WALL_GAP && x + width > self.patch_width - WALL_GAP {
                x = WALL_GAP;
                y += self.tile_height + WALL_GAP;
            }
            let held = Held { item, x, y, width };
            match self.held.get(position) {
                Some(shown) if *shown == held => {
                    if position > high + 1 {
                        break;
                    }
                }
                Some(_) => {
                    self.held[position] = held;
                    self.placed.set_row_data(position, Self::placed(held));
                }
                None => {
                    self.held.push(held);
                    self.placed.push(Self::placed(held));
                }
            }
            x += width + WALL_GAP;
        }
    }

    fn placed(held: Held) -> PlacedTile {
        PlacedTile {
            tile: wall_tile(held.item),
            x: held.x,
            y: held.y,
            width: held.width,
        }
    }
}

/// The cards patch: the cards in the order the frame's move draws them.
struct CardStack {
    cards: Cards,
    model: Rc<VecModel<PatchCard>>,
    moves: Option<u32>,
}

impl CardStack {
    fn card(&self, id: usize) -> PatchCard {
        let tiles = self.cards.tiles();
        PatchCard {
            id: id as i32,
            title: patchwork_card_title(id).into(),
            header: color(patchwork_card_rgb(id)),
            tiles: ModelRc::new(VecModel::from(
                (0..tiles)
                    .map(|tile| wall_tile(patchwork_card_tile(id, tile, tiles)))
                    .collect::<Vec<_>>(),
            )),
        }
    }

    // When a move begins a card comes in at the front or the last one has left
    // at the end. The cards that stay keep their rows, and so their tiles.
    fn advance(&mut self, frame: u32) {
        let moves = frame / PATCHWORK_CARD_FRAMES;
        if self.moves.replace(moves) == Some(moves) {
            return;
        }
        let begun = moves * PATCHWORK_CARD_FRAMES;
        let ids: Vec<usize> = (0..self.cards.places())
            .filter_map(|place| self.cards.place(place, begun))
            .map(|card| card.id)
            .collect();
        let id_at = |model: &VecModel<PatchCard>, row: usize| {
            model.row_data(row).map(|card| card.id as usize)
        };
        // Slint frees the elements of a removed row through the renderer,
        // which is busy while the frame loop runs: the removal runs on the
        // event loop.
        let oldest = ids.last().copied().unwrap_or(0);
        let left = (0..self.model.row_count())
            .filter(|row| id_at(&self.model, *row).is_some_and(|id| id < oldest))
            .count();
        if left > 0 {
            let model = self.model.clone();
            slint::Timer::single_shot(Duration::ZERO, move || {
                for _ in 0..left {
                    model.remove(model.row_count() - 1);
                }
            });
        }
        let front = id_at(&self.model, 0);
        let new = ids
            .iter()
            .filter(|id| front.is_none_or(|front| **id > front));
        for (at, id) in new.enumerate() {
            self.model.insert(at, self.card(*id));
        }
    }
}

/// What the frame loop writes after each frame: the patches' models.
struct Patches {
    recolor: Rc<VecModel<Color>>,
    recolor_turn: InTurn,
    nests: Rc<VecModel<SharedString>>,
    nest_turn: InTurn,
    pile: Rc<VecModel<Color>>,
    pile_opaque: usize,
    pile_colors: Vec<Color>,
    cards: CardStack,
    reorder: Reorder,
    paragraph: String,
    words: usize,
    changed_word: usize,
    scroll_rows: usize,
    visible_rows: usize,
    pitch: f32,
    chat: ChatModel,
}

impl Patches {
    // Writes frame `frame`: the rows that differ from frame `frame - 1`.
    fn advance(&mut self, window: &PatchworkWindow, frame: u32) {
        let columns = self.recolor_turn.columns;
        let row = row_in_turn(frame, self.recolor_turn.rows);
        for index in row * columns..(row + 1) * columns {
            self.recolor.set_row_data(
                index,
                color(patchwork_recolor_rgb(index, frame, self.recolor_turn)),
            );
        }

        self.cards.advance(frame);
        self.reorder.advance(frame);

        let columns = self.nest_turn.columns;
        let row = row_in_turn(frame, self.nest_turn.rows);
        for index in row * columns..(row + 1) * columns {
            self.nests.set_row_data(
                index,
                patchwork_deep_value(index, frame, self.nest_turn).into(),
            );
        }

        window.set_scroll_offset(
            -(patchwork_scroll_first(frame, self.scroll_rows, self.visible_rows) as f32)
                * self.pitch,
        );

        self.write_word(self.changed_word, self.changed_word);
        self.changed_word = (frame as usize).wrapping_mul(37) % self.words.max(1);
        self.write_word(self.changed_word, frame as usize % 100_000);
        window.set_paragraph(self.paragraph.as_str().into());

        let opaque = patchwork_pile_opaque(frame, self.pile.row_count());
        if opaque != self.pile_opaque {
            self.pile
                .set_row_data(self.pile_opaque, self.pile_colors[self.pile_opaque]);
            self.pile_opaque = opaque;
            self.pile
                .set_row_data(opaque, color(PATCHWORK_PILE_OPAQUE_RGB));
        }

        if let Some(image) = self.chat.advance(frame) {
            window.set_chat_gif(image);
        }
    }

    // Writes word `word` of the paragraph as the label of tile `label`.
    fn write_word(&mut self, word: usize, label: usize) {
        let mut digits = [b'0'; 5];
        let mut rest = label % 100_000;
        for digit in digits.iter_mut().rev() {
            *digit += (rest % 10) as u8;
            rest /= 10;
        }
        if let Ok(digits) = std::str::from_utf8(&digits) {
            self.paragraph.replace_range(word * 6..word * 6 + 5, digits);
        }
    }
}

/// How a window divides into patches and tiles.
struct Layout {
    patch_width: f32,
    patch_height: f32,
    tiles: PatchTiles,
    /// The nests or charts across and down a patch.
    pairs: (usize, usize),
    /// The tiles of the pile across and down a patch.
    pile: (usize, usize),
}

impl Layout {
    fn new(tier: WallTier, size: slint::LogicalSize) -> Self {
        let patch_width = ((size.width - CHAT_WIDTH) / PATCH_COLUMNS as f32).floor();
        let patch_height = ((size.height - 56.0 - WALL_CLOCK_HEIGHT) / PATCH_ROWS as f32).floor();
        let tiles = PatchTiles::new(tier, patch_width, patch_height);
        Self {
            patch_width,
            patch_height,
            tiles,
            pairs: patchwork_pairs(tiles.columns, tiles.rows),
            pile: patchwork_pile_grid(tiles.columns, tiles.rows),
        }
    }

    // Hands the sizes to the markup and the models that never change: the
    // tiles and the charts.
    fn show(&self, window: &PatchworkWindow) {
        let tiles = self.tiles;
        let (label_size, value_size) = wall_text_size(tiles.height);
        let metrics = window.global::<Metrics>();
        metrics.set_patch_width(self.patch_width);
        metrics.set_patch_height(self.patch_height);
        metrics.set_tile_width(tiles.width);
        metrics.set_tile_height(tiles.height);
        metrics.set_columns(tiles.columns as i32);
        metrics.set_rows(tiles.rows as i32);
        metrics.set_label_size(label_size);
        metrics.set_value_size(value_size);
        metrics.set_dot(wall_dot_size(tiles.height));
        metrics.set_pad(wall_padding(tiles.height));
        metrics.set_palette(ModelRc::new(VecModel::from(
            PALETTE_RGB.map(color).to_vec(),
        )));
        metrics.set_pair_columns(self.pairs.0 as i32);
        metrics.set_pair_rows(self.pairs.1 as i32);
        metrics.set_pile_columns(self.pile.0 as i32);
        metrics.set_pile_count((self.pile.0 * self.pile.1) as i32);

        window.set_tiles(ModelRc::new(VecModel::from(
            (0..tiles.count()).map(wall_tile).collect::<Vec<_>>(),
        )));
        let (width, height) = (
            patchwork_double(tiles.width),
            patchwork_double(tiles.height),
        );
        window.set_charts(ModelRc::new(VecModel::from(
            (0..self.pairs.0 * self.pairs.1)
                .map(|index| ChartData {
                    path: chart_path(index, width, height),
                    color: color(patchwork_chart_rgb(index)),
                })
                .collect::<Vec<_>>(),
        )));
    }
}

impl Patches {
    // Makes the models of the patches a frame changes, hands them to
    // `window` and writes frame 0.
    fn new(window: &PatchworkWindow, layout: &Layout) -> Self {
        let tiles = layout.tiles;
        let recolor_turn = tiles.in_turn();
        let recolor = Rc::new(VecModel::from(
            (0..tiles.count())
                .map(|index| color(patchwork_recolor_rgb(index, 0, recolor_turn)))
                .collect::<Vec<_>>(),
        ));
        window.set_recolor_fills(recolor.clone().into());

        let cards = Cards::new(tiles.columns, tiles.rows, tiles.height, layout.patch_height);
        window.global::<Metrics>().on_card_top(move |id, frame| {
            cards
                .top(id as usize, frame as u32)
                .unwrap_or(-cards.height)
        });
        let cards = CardStack {
            cards,
            model: Rc::new(VecModel::default()),
            moves: None,
        };
        window.set_cards(cards.model.clone().into());

        let reorder = Reorder::new(tiles, layout.patch_width);
        window.set_placed(reorder.placed.clone().into());

        let (pair_columns, pair_rows) = layout.pairs;
        let nest_turn = InTurn {
            columns: pair_columns,
            rows: pair_rows,
        };
        let nests = Rc::new(VecModel::from(
            (0..pair_columns * pair_rows)
                .map(|index| SharedString::from(patchwork_deep_value(index, 0, nest_turn)))
                .collect::<Vec<_>>(),
        ));
        window.set_nests(nests.clone().into());

        let scroll_rows = patchwork_scroll_rows(tiles.columns);
        window.set_scroll_rows(ModelRc::new(ScrollRows {
            columns: tiles.columns,
            rows: scroll_rows,
        }));

        let (pile_columns, pile_rows) = layout.pile;
        let pile_colors: Vec<Color> = (0..pile_columns * pile_rows)
            .map(|index| {
                let [red, green, blue] = wall_fill_rgb(index);
                Color::from_argb_u8(PATCHWORK_PILE_ALPHA, red, green, blue)
            })
            .collect();
        let pile = Rc::new(VecModel::from(pile_colors.clone()));
        let pile_opaque = patchwork_pile_opaque(0, pile_colors.len());
        pile.set_row_data(pile_opaque, color(PATCHWORK_PILE_OPAQUE_RGB));
        window.set_pile_fills(pile.clone().into());

        let chat = ChatModel::new();
        window.set_chat_items(chat.items());
        window.set_chat_status(chat.status());
        window.set_chat_spinner(ChatModel::spinner());

        let words = patchwork_paragraph_words(tiles.count());
        let mut patches = Self {
            recolor,
            recolor_turn,
            nests,
            nest_turn,
            pile,
            pile_opaque,
            pile_colors,
            cards,
            reorder,
            paragraph: patchwork_paragraph(0, words),
            words,
            changed_word: 0,
            scroll_rows,
            visible_rows: tiles.rows,
            pitch: tiles.height + WALL_GAP,
            chat,
        };
        window.global::<WallClock>().set_text(wall_clock(0).into());
        patches.advance(window, 0);
        patches
    }
}

// Moves on to the next frame after every frame drawn, up to the frame
// `freeze` names, and writes what the frame changes.
fn drive(window: &PatchworkWindow, mut patches: Patches, freeze: u32) -> Result<(), PlatformError> {
    let weak = window.as_weak();
    let mut frame = FrameIndex::new(freeze);
    window
        .window()
        .set_rendering_notifier(move |state, _| {
            if !matches!(state, RenderingState::AfterRendering) {
                return;
            }
            let Some(window) = weak.upgrade() else { return };
            if frame.frozen() {
                return;
            }
            frame.advance();
            let clock = window.global::<WallClock>();
            clock.set_frame(frame.get() as i32);
            clock.set_text(wall_clock(frame.get()).into());
            patches.advance(&window, frame.get());
        })
        .map_err(|error| PlatformError::Other(format!("no rendering notifier: {error}")))
}

/// Shows the patchwork in a window of `size`.
pub fn run(launch: Launch, size: slint::LogicalSize) -> Result<(), PlatformError> {
    let window = PatchworkWindow::new()?;
    window.window().set_size(size);
    let layout = Layout::new(wall_tier(launch.tier), size);
    layout.show(&window);
    let patches = Patches::new(&window, &layout);
    drive(&window, patches, launch.freeze)?;
    window.run()
}
