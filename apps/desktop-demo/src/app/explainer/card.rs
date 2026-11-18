// The card of live text on the silk section's first page: words printed on
// cotton card stock, which select and copy, on a board that stands off the
// wall. A selection on it is a stroke of red highlighter.

use std::sync::{Arc, OnceLock};

use cranpose_core::{remember, CompositionLocalProvider};
use cranpose_ui::{
    composable, local_text_selection_colors,
    space::{Mat4, Material, Mesh, MeshSurface, Shaded, Vec3},
    widgets::SelectionContainer,
    Box, BoxSpec, Color, Column, ColumnSpec, LinearArrangement, Modifier, Size, Text,
    TextSelectionColors,
};

use super::lettering::{headline, text};

// The paper as the content shows it; the shader lays its tooth on it.
const PAPER: Color = Color(0.97, 0.95, 0.91, 1.0);
// The board's sides.
const EDGE: Color = Color(0.66, 0.66, 0.68, 1.0);
// The colour the content marks a selection with: the paper shows it as
// highlighter.
const MARKER: Color = Color(1.0, 0.45, 0.40, 1.0);
// How thick the board is.
const THICKNESS: f32 = 5.0;
const PAPER_WGSL: &str = cranpose_ui::include_wgsl!("noise.wgsl", "paper.wgsl");

// A card `card` big whose face stands `depth` deep, printed in `ink`,
// moved `slide()` across from where it is laid out.
#[composable(no_skip)]
pub(super) fn WidgetCard(
    card: Size,
    (u, depth, ink): (f32, f32, Color),
    slide: impl Fn() -> f32 + Copy + 'static,
) {
    let (plane, block) =
        remember(|| (Arc::new(Mesh::plane()), Arc::new(Mesh::cuboid()))).with(Clone::clone);
    let paper = paper(card, ink);
    let marker = TextSelectionColors {
        handle_color: MARKER,
        background_color: MARKER,
    };
    Box(
        Modifier::empty().required_size(card),
        BoxSpec::default(),
        move || {
            let block = Arc::clone(&block);
            Box(
                Modifier::empty()
                    .required_size(Size::new(16.0, 16.0))
                    .mesh_surface(move || MeshSurface {
                        mesh: Arc::clone(&block),
                        // A hair behind the face, so the two never fight.
                        transform: Mat4::scaling(Vec3::new(card.width, card.height, THICKNESS))
                            .then(&Mat4::translation(Vec3::new(
                                slide() + card.width * 0.5,
                                card.height * 0.5,
                                depth + THICKNESS * 0.5 + 1.0,
                            ))),
                        material: Material::Lit,
                        casts_shadow: true,
                        solid: true,
                    })
                    .background(EDGE),
                BoxSpec::default(),
                || {},
            );
            let (plane, paper) = (Arc::clone(&plane), paper.clone());
            CompositionLocalProvider(
                [local_text_selection_colors().provides(marker)],
                move || {
                    let (plane, paper) = (Arc::clone(&plane), paper.clone());
                    SelectionContainer(
                        Modifier::empty()
                            .required_size(card)
                            .mesh_surface(move || MeshSurface {
                                mesh: Arc::clone(&plane),
                                transform: Mat4::scaling(Vec3::new(card.width, card.height, 1.0))
                                    .then(&Mat4::translation(Vec3::new(slide(), 0.0, depth))),
                                material: paper.clone(),
                                casts_shadow: true,
                                solid: true,
                            })
                            .background(PAPER)
                            .padding(40.0 * u),
                        move || {
                            Column(
                                Modifier::empty(),
                                ColumnSpec::new()
                                    .vertical_arrangement(LinearArrangement::SpacedBy(16.0 * u)),
                                move || {
                                    Text("Widgets", Modifier::empty(), headline(52.0 * u, ink));
                                    Text(
                                "Ready-made buttons, fields and links. You can select these \
                                 words, but they can't wave like silk.",
                                Modifier::empty(),
                                text(30.0 * u, ink),
                            );
                                },
                            );
                        },
                    );
                },
            );
        },
    );
}

// Cotton card stock `card` big, printed in `ink`.
fn paper(card: Size, ink: Color) -> Material {
    static SHADER: OnceLock<Arc<str>> = OnceLock::new();
    let mut params = [0.0; Shaded::PARAMS];
    params[..2].copy_from_slice(&[card.width, card.height]);
    for (first, color) in [(4, PAPER), (8, ink), (12, MARKER)] {
        params[first..first + 3].copy_from_slice(&[color.0, color.1, color.2]);
    }
    Material::Shaded(Shaded {
        shader: Arc::clone(SHADER.get_or_init(|| Arc::from(PAPER_WGSL))),
        params,
    })
}
