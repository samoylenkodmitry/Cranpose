//! The still wall's tile, which the wall and every patch of the patchwork
//! show: its fill, its label in five bold digits, a dot and its value.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use floem::{
    View, ViewId,
    context::PaintCx,
    kurbo::{Point, Rect},
    peniko::{Color, Image},
    text::TextLayout,
};
use floem_renderer::{Img, Renderer as _};
use perf_data::*;

use crate::{
    raster::Raster,
    rgb,
    surface::Surface,
    text::{Face, Type},
};

/// The line height of a tile's texts, as a multiple of their size.
const LINE: f32 = 1.3;

/// The sizes of the tiles of one height, and the text they have shaped.
pub struct TileKit {
    pub height: f32,
    label: Type,
    value: Type,
    dot: f32,
    pad: f32,
    labels: RefCell<HashMap<usize, Rc<TextLayout>>>,
    values: RefCell<HashMap<usize, Rc<TextLayout>>>,
}

/// A tile's text, from the kit's shared layouts.
pub struct TileText {
    label: Rc<TextLayout>,
    value: Rc<TextLayout>,
}

impl TileKit {
    pub fn new(height: f32) -> Rc<Self> {
        let (label, value) = wall_text_size(height);
        let ink = rgb(WALL_INK_RGB);
        Rc::new(Self {
            height,
            label: Type::new(label, LINE, Face::Bold, ink),
            value: Type::new(value, LINE, Face::Regular, ink),
            dot: wall_dot_size(height),
            pad: wall_padding(height),
            labels: RefCell::default(),
            values: RefCell::default(),
        })
    }

    /// The text of tile `index`. Two tiles with equal digits share a layout.
    pub fn text(&self, index: usize) -> TileText {
        let label = self
            .labels
            .borrow_mut()
            .entry(index)
            .or_insert_with(|| Rc::new(self.label.shape(&wall_label(index), None)))
            .clone();
        let digits = wall_value(index);
        let key = digits.parse().unwrap_or_default();
        let value = self
            .values
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| Rc::new(self.value.shape(&digits, None)))
            .clone();
        TileText { label, value }
    }

    /// The fill of a tile at `(x, y)`, `width` by `height` large.
    pub fn paint_fill(
        &self,
        surface: &mut impl Surface,
        (x, y): (f64, f64),
        (width, height): (f32, f32),
        fill: Color,
    ) {
        surface.fill_rounded(
            (x, y, x + f64::from(width), y + f64::from(height)),
            f64::from(WALL_CORNER),
            fill,
        );
    }

    /// The dot and the texts of tile `index` at `(x, y)`, which no frame
    /// changes: the text of a tile is its index.
    pub fn paint_text(
        &self,
        surface: &mut impl Surface,
        (x, y): (f64, f64),
        index: usize,
        text: &TileText,
    ) {
        let (left, top) = (x as f32 + self.pad, y as f32 + self.pad);
        surface.text(&text.label, Point::new(f64::from(left), f64::from(top)));
        // The dot and the value share a row as tall as the taller of the two.
        let row_top = top + self.label.line_height();
        let row = self.dot.max(self.value.line_height());
        surface.fill_circle(
            Point::new(
                f64::from(left + self.dot / 2.0),
                f64::from(row_top + row / 2.0),
            ),
            f64::from(self.dot / 2.0),
            rgb(wall_dot_rgb(index)),
        );
        surface.text(
            &text.value,
            Point::new(
                f64::from(left + self.dot * 1.5),
                f64::from(row_top + (row - self.value.line_height()) / 2.0),
            ),
        );
    }

    /// Tile `index` at `(x, y)`, `width` by `height` large, filled with
    /// `fill`, its text set for the kit's height.
    pub fn paint(
        &self,
        surface: &mut impl Surface,
        at: (f64, f64),
        size: (f32, f32),
        fill: Color,
        index: usize,
        text: &TileText,
    ) {
        self.paint_fill(surface, at, size, fill);
        self.paint_text(surface, at, index, text);
    }
}

/// `columns` by `rows` tiles of `width`, `WALL_GAP` apart and `margin` from
/// the edges.
pub struct TileGrid {
    kit: Rc<TileKit>,
    columns: usize,
    width: f32,
    margin: f32,
    text: Vec<TileText>,
    index: Box<dyn Fn(usize) -> usize>,
    fill: Box<dyn Fn(usize) -> Color>,
}

impl TileGrid {
    /// The tile at place `place`, counted along the rows, is tile
    /// `index(place)` and has the fill `fill(index)` in each frame. The
    /// tiles start `margin` from the view's edges.
    pub fn new(
        kit: &Rc<TileKit>,
        (columns, rows): (usize, usize),
        (width, margin): (f32, f32),
        index: impl Fn(usize) -> usize + 'static,
        fill: impl Fn(usize) -> Color + 'static,
    ) -> Self {
        Self {
            kit: kit.clone(),
            columns,
            width,
            margin,
            text: (0..columns * rows)
                .map(|place| kit.text(index(place)))
                .collect(),
            index: Box::new(index),
            fill: Box::new(fill),
        }
    }
}

impl TileGrid {
    fn at(&self, place: usize) -> (f64, f64) {
        (
            f64::from(self.margin)
                + (place % self.columns) as f64 * f64::from(self.width + WALL_GAP),
            f64::from(self.margin)
                + (place / self.columns) as f64 * f64::from(self.kit.height + WALL_GAP),
        )
    }

    /// Draws the fills of the tiles for the frame `fill` follows.
    fn draw_fills(&self, surface: &mut impl Surface) {
        for place in 0..self.text.len() {
            let fill = (self.fill)((self.index)(place));
            self.kit
                .paint_fill(surface, self.at(place), (self.width, self.kit.height), fill);
        }
    }

    /// Draws the dots and texts of the tiles.
    fn draw_text(&self, surface: &mut impl Surface) {
        for (place, text) in self.text.iter().enumerate() {
            self.kit
                .paint_text(surface, self.at(place), (self.index)(place), text);
        }
    }

    /// Draws every tile for the frame `fill` follows.
    pub fn draw(&self, surface: &mut impl Surface) {
        self.draw_fills(surface);
        self.draw_text(surface);
    }

    /// The size of the part of the view the tiles cover, from its corner.
    fn extent(&self) -> (f32, f32) {
        let rows = self.text.len().div_ceil(self.columns);
        (
            self.margin + self.columns as f32 * (self.width + WALL_GAP),
            self.margin + rows as f32 * (self.kit.height + WALL_GAP),
        )
    }

    /// The grid as a view that paints the fills in each frame. The dots and
    /// the texts never change, so they are drawn once into a bitmap that lies
    /// over the fills: Vello draws a few tens of thousands of shapes and
    /// glyphs a frame, and the fills are one shape each.
    pub fn field(self) -> TileField {
        TileField {
            id: ViewId::new(),
            grid: self,
            text: None,
        }
    }
}

/// A grid of tiles that paints itself.
pub struct TileField {
    id: ViewId,
    grid: TileGrid,
    text: Option<Image>,
}

impl View for TileField {
    fn id(&self) -> ViewId {
        self.id
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        self.grid.draw_fills(cx);
        let size = self.grid.extent();
        let image = self.text.get_or_insert_with(|| {
            let mut raster = Raster::new(size, cx.scale() as f32, Color::TRANSPARENT);
            self.grid.draw_text(&mut raster);
            raster.into_image()
        });
        cx.draw_img(
            Img {
                img: image.clone(),
                hash: &[],
            },
            Rect::new(0.0, 0.0, f64::from(size.0), f64::from(size.1)),
        );
    }
}
