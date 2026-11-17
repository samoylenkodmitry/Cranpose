// The caption of "How it works": a headline, a line on what a composable
// and a mesh are, and the four steps the scroll walks. The step the walk is
// on reads at full strength with an orange numeral; the others are dimmed.

use cranpose_core::derivedStateOf;
use cranpose_ui::{
    composable,
    text::{AnnotatedString, FontFamily, FontWeight, SpanStyle},
    widgets::SelectionContainer,
    Color, Column, ColumnSpec, LinearArrangement, Modifier, Row, RowSpec, Size, Text,
};

use super::{
    figure::Plan,
    lettering::{headline, text, STRONG_FAMILY},
    walk::Walk,
};

const CREAM: Color = Color(1.0, 0.969, 0.901, 1.0);
const DIM_CREAM: Color = Color(0.62, 0.70, 0.62, 1.0);
const ORANGE: Color = Color(1.0, 0.63, 0.19, 1.0);
const STEPS: [(&str, &str); 4] = [
    (
        "Lay it out.",
        " Cranpose arranges the words, as on every screen. It knows where each one is.",
    ),
    (
        "Take its picture.",
        " It draws the result into a texture. Change a word and it draws it again.",
    ),
    ("Bend it.", " A mesh can carry that picture on any shape."),
    (
        "Point back.",
        " When you point at the bent copy, Cranpose finds that spot on the flat copy. \
         The words there take the pointer.",
    ),
];

// The caption, left of the figure planned by `plan` on a stage of `size`,
// lighting the step `walk` is on.
#[composable(no_skip)]
pub(super) fn HowSteps(size: Size, plan: Plan, walk: Walk) {
    let u = plan.u;
    let current = derivedStateOf(move || ((walk.progress() * 4.0) as usize).min(3)).get();
    let left = (size.width * 0.5 - 644.0 * u).max(16.0);
    SelectionContainer(
        Modifier::empty()
            .offset(left, 0.0)
            .required_size(Size::new(448.0 * u, size.height)),
        move || {
            Column(
                Modifier::empty().fill_max_size(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::Center),
                move || {
                    Text("How it works", Modifier::empty(), headline(68.0 * u, CREAM));
                    Text(
                        "A composable is code that lays out words and controls. A mesh is a \
                         shape in a 3D scene.",
                        Modifier::empty().padding_each(0.0, 20.0 * u, 0.0, 32.0 * u),
                        text(22.0 * u, CREAM),
                    );
                    for (index, (lead, rest)) in STEPS.into_iter().enumerate() {
                        Step(index, (lead, rest), u, index == current);
                    }
                },
            );
        },
    );
}

#[composable(no_skip)]
fn Step(index: usize, (lead, rest): (&'static str, &'static str), u: f32, current: bool) {
    let ink = if current { CREAM } else { DIM_CREAM };
    Row(
        Modifier::empty().padding_each(0.0, 0.0, 0.0, 20.0 * u),
        RowSpec::default(),
        move || {
            let mut numeral = headline(36.0 * u, if current { ORANGE } else { DIM_CREAM });
            numeral.paragraph_style.line_height = cranpose_ui::text::TextUnit::Sp(36.0 * u);
            Text(
                (index + 1).to_string(),
                Modifier::empty().width(48.0 * u),
                numeral,
            );
            let mut bold = SpanStyle::default();
            bold.font_family = Some(FontFamily::named(STRONG_FAMILY));
            bold.font_weight = Some(FontWeight::BOLD);
            let words = AnnotatedString::builder()
                .push_style(bold)
                .append(lead)
                .pop()
                .append(rest)
                .to_annotated_string();
            let mut style = text(22.0 * u, ink);
            style.paragraph_style.line_height = cranpose_ui::text::TextUnit::Sp(22.0 * u * 1.34);
            Text(words, Modifier::empty().width(400.0 * u), style);
        },
    );
}
