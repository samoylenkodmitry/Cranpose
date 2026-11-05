// One of the backrooms' own source files, printed on a stretch of wall in
// the room's light. It scrolls: the wheel or a drag moves the lines, of
// which only those on the wall are laid out.

use std::{rc::Rc, sync::Arc};

use cranpose::{
    widgets::{LazyColumn, LazyColumnSpec},
    LazyItems,
};
use cranpose_core::LaunchedEffect;
use cranpose_foundation::lazy::{rememberLazyListState, LazyListScope, LazyListState};
use cranpose_ui::{
    composable,
    space::{Mat4, MeshSurface},
    text::{FontFamily, FontWeight, TextUnit},
    Box, BoxSpec, Color, Column, ColumnSpec, Modifier, SharedText, Size, Text, TextStyle,
};

use super::{
    finish::{Finish, Sight},
    place::spot,
    plan,
    shelf::{Page, FILES},
};

// How far the print stands off the wall.
const OUT: f32 = 0.6;
const INK: Color = Color(0.16, 0.12, 0.08, 0.88);

fn print(weight: FontWeight) -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.font_size = TextUnit::Sp(13.0);
    style.span_style.color = Some(INK);
    style.span_style.font_family = Some(FontFamily::Monospace);
    style.span_style.font_weight = Some(weight);
    style
}

#[composable]
pub(super) fn Printout(sight: Sight, printout: plan::Printout, size: (f32, f32)) {
    let panel = printout.panel;
    let transform = panel
        .transform(size)
        .then(&Mat4::translation(panel.side.inward() * OUT));
    let content = Size {
        width: panel.to - panel.from,
        height: panel.top - panel.bottom,
    };
    let name = FILES[printout.file.min(FILES.len() - 1)];
    let page = sight.level.shelf.page(printout.file);
    // The wall wants its page once it shows: an effect, as composing only
    // describes what shows.
    LaunchedEffect(printout.file, move |_| {
        if page.get_non_reactive() == Page::Blank {
            page.set(Page::Wanted);
        }
    });
    let state = rememberLazyListState();
    let plane = Arc::clone(&sight.level.meshes.plane);
    Box(
        spot(content).mesh_surface(move || MeshSurface {
            mesh: Arc::clone(&plane),
            transform,
            material: sight.material(Finish::Content, &transform),
            casts_shadow: false,
            solid: false,
        }),
        BoxSpec::default(),
        move || {
            Column(
                Modifier::empty().fill_max_size().padding(10.0),
                ColumnSpec::default(),
                move || {
                    Text(
                        format!("// {name}"),
                        Modifier::empty(),
                        print(FontWeight::BOLD),
                    );
                    match page.get() {
                        Page::Blank | Page::Wanted => {
                            Text(
                                "// printing...",
                                Modifier::empty(),
                                print(FontWeight::NORMAL),
                            );
                        }
                        Page::Torn(reason) => {
                            Text(reason, Modifier::empty(), print(FontWeight::NORMAL));
                        }
                        Page::Printed(lines) => Lines(state, lines),
                    }
                },
            );
        },
    );
}

// The printed lines, as many as the wall shows laid out.
#[composable]
fn Lines(state: LazyListState, lines: Rc<[SharedText]>) {
    LazyColumn(
        Modifier::empty().weight(1.0),
        state,
        LazyColumnSpec::new(),
        move |scope| {
            let lines = Rc::clone(&lines);
            scope.items(
                LazyItems::new(lines.len()).key(|index: usize| index as u64),
                move |index| {
                    if let Some(line) = lines.get(index) {
                        Text(line.clone(), Modifier::empty(), print(FontWeight::NORMAL));
                    }
                },
            );
        },
    );
}
