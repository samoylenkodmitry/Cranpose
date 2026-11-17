// The pool's writing lane: a sunk panel of white tiles in a frame one tile
// wide, with a live text field in it. Its first words stay until someone
// types. It is painted in the pool's key colours, which the floor's shader
// turns into tiles: the frame yellow, the panel cyan, the caret orange.

use cranpose_foundation::text::TextFieldState;
use cranpose_ui::{
    composable,
    text::{prepare_text_layout, AnnotatedString, TextLayoutOptions, TextUnit},
    widgets::{BasicTextFieldOptions, BasicTextFieldWithOptions},
    Box, BoxSpec, Color, Modifier, MutableInteractionSource, Point, RoundedCornerShape, TextStyle,
};

use super::lettering::headline;

pub(super) const FIRST_WORDS: &str = "Write something here";
pub(super) const FRAME: Color = Color(1.0, 1.0, 0.0, 1.0);
pub(super) const PANEL: Color = Color(0.0, 1.0, 1.0, 1.0);
pub(super) const CARET: Color = Color(1.0, 0.35, 0.0, 1.0);
// The lane's height in tiles, its frame included.
const ROWS: f32 = 6.0;

// Where the lane lies on the floor and how its words are set.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Lane {
    // The lane's top left corner, its frame included.
    pub top_left: Point,
    pub width: f32,
    pub tile: f32,
    pub style: TextStyle,
}

impl Lane {
    pub(super) fn new(top_left: Point, width: f32, (u, tile): (f32, f32), ink: Color) -> Self {
        let mut style = headline(86.0 * u, ink);
        style.paragraph_style.line_height = TextUnit::Sp(4.0 * tile);
        Self {
            top_left,
            width,
            tile,
            style,
        }
    }

    // Where the field's words start: inside the frame and a tile of
    // padding across, and inside the frame down.
    fn text_origin(&self) -> Point {
        Point::new(
            self.top_left.x + 2.0 * self.tile,
            self.top_left.y + self.tile,
        )
    }

    fn text_width(&self) -> f32 {
        (self.width - 4.0 * self.tile).max(1.0)
    }

    // Where the caret of `field` stands on the floor: the middle of its
    // line.
    pub(super) fn caret_point(&self, field: TextFieldState) -> Point {
        let offset = field.selection().end;
        let caret = field.with_text(|text| {
            let layout = prepare_text_layout(
                &AnnotatedString::from(text),
                &self.style,
                TextLayoutOptions::default(),
                Some(self.text_width()),
            );
            layout.layout.cursor_rect(offset.min(text.len()))
        });
        let origin = self.text_origin();
        Point::new(origin.x + caret.x, origin.y + caret.y + caret.height * 0.5)
    }
}

// The lane over `lane.width`, with `field` in it; `hover` follows the
// pointer over it.
#[composable(no_skip)]
pub(super) fn WritingLane(lane: Lane, field: TextFieldState, hover: MutableInteractionSource) {
    let tile = lane.tile;
    let options = BasicTextFieldOptions {
        text_style: lane.style.clone(),
        cursor_color: CARET,
        ..BasicTextFieldOptions::default()
    };
    Box(
        Modifier::empty()
            .width(lane.width)
            .height_in(ROWS * tile, f32::INFINITY)
            .background(PANEL)
            .border(tile, FRAME, RoundedCornerShape::uniform(0.0))
            .hoverable(hover, true)
            .padding_symmetric(2.0 * tile, tile),
        BoxSpec::default(),
        move || {
            BasicTextFieldWithOptions(field, Modifier::empty().fill_max_width(), options.clone());
        },
    );
}
