// One of the backrooms' own source files, printed on a stretch of wall in
// the room's light. It scrolls: the wheel or a drag moves the lines, of
// which only those on the wall are laid out.

use std::{rc::Rc, sync::Arc};

use cranpose::{
    widgets::{LazyColumn, LazyColumnSpec},
    LazyItems,
};
use cranpose_core::remember;
use cranpose_foundation::lazy::{rememberLazyListState, LazyListScope};
use cranpose_ui::{
    composable,
    space::{Mat4, Mesh, MeshSurface},
    text::{FontFamily, FontWeight, TextUnit},
    Box, BoxSpec, Color, Modifier, Size, Text, TextStyle,
};

use super::{
    finish::{Finish, Lamps, Sight},
    place::spot,
    plan::{self, PRINTS},
};

// How far the print stands off the wall.
const OUT: f32 = 0.6;
const INK: Color = Color(0.16, 0.12, 0.08, 0.88);

const FILES: [(&str, &str); PRINTS as usize] = [
    ("anomaly.rs", include_str!("anomaly.rs")),
    ("figure.rs", include_str!("figure.rs")),
    ("finish.rs", include_str!("finish.rs")),
    ("finish.wgsl", include_str!("finish.wgsl")),
    ("hud.rs", include_str!("hud.rs")),
    ("journey.rs", include_str!("journey.rs")),
    ("notes.rs", include_str!("notes.rs")),
    ("place.rs", include_str!("place.rs")),
    ("plan.rs", include_str!("plan.rs")),
    ("printout.rs", include_str!("printout.rs")),
    ("room.rs", include_str!("room.rs")),
    ("tab.rs", include_str!("tab.rs")),
    ("walker.rs", include_str!("walker.rs")),
];

fn print(weight: FontWeight) -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.font_size = TextUnit::Sp(13.0);
    style.span_style.color = Some(INK);
    style.span_style.font_family = Some(FontFamily::Monospace);
    style.span_style.font_weight = Some(weight);
    style
}

#[composable(no_skip)]
pub(super) fn Printout(
    sight: Sight,
    plane: Arc<Mesh>,
    lamps: Lamps,
    printout: plan::Printout,
    size: (f32, f32),
) {
    let panel = printout.panel;
    let transform = panel
        .transform(size)
        .then(&Mat4::translation(panel.side.inward() * OUT));
    let content = Size {
        width: panel.to - panel.from,
        height: panel.top - panel.bottom,
    };
    let (name, source) = FILES[printout.file.min(FILES.len() - 1)];
    let lines = remember(move || source.lines().collect::<Rc<[&'static str]>>()).with(Rc::clone);
    let state = rememberLazyListState();
    Box(
        spot(content).mesh_surface(move || MeshSurface {
            mesh: Arc::clone(&plane),
            transform,
            material: sight.material(Finish::Content, &transform, lamps),
            casts_shadow: false,
            solid: false,
        }),
        BoxSpec::default(),
        move || {
            let lines = Rc::clone(&lines);
            LazyColumn(
                Modifier::empty().fill_max_size().padding(10.0),
                state,
                LazyColumnSpec::new(),
                move |scope| {
                    let lines = Rc::clone(&lines);
                    scope.items(
                        LazyItems::new(lines.len() + 1).key(|index: usize| index as u64),
                        move |index| match index.checked_sub(1) {
                            None => {
                                Text(
                                    format!("// {name}"),
                                    Modifier::empty(),
                                    print(FontWeight::BOLD),
                                );
                            }
                            Some(line) => {
                                Text(
                                    lines.get(line).copied().unwrap_or_default(),
                                    Modifier::empty(),
                                    print(FontWeight::NORMAL),
                                );
                            }
                        },
                    );
                },
            );
        },
    );
}
