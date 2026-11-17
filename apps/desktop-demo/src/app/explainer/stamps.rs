// The demo gallery: a perforated sheet of postage stamps lying on marigold
// paper in window light. Each stamp shows one of the demo's tabs and opens
// it. The ground, the sheet and every stamp are composables drawn on meshes
// in the stage's scene, so the stamps throw real shadows when they lift.
// While the page comes in, the stamps settle onto the sheet one after
// another.

use std::{
    rc::Rc,
    sync::{Arc, OnceLock},
};

use cranpose_core::{key, remember, CompositionLocalProvider};
use cranpose_ui::{
    composable, local_text_selection_colors,
    space::{Mat4, Material, Mesh, MeshSurface, Shaded, Vec3},
    text::TextAlign,
    widgets::SelectionContainer,
    Box, BoxSpec, Brush, Canvas, Color, DrawStyle, Modifier, Path, Point, Rect, ScrollState, Size,
    Text, TextSelectionColors,
};

use super::{
    curl::{Bend, CurlMeshes},
    lettering::headline,
    perforation::{perforated, Winding},
    stage::{SceneStage, STAGE_LIGHT},
    stamp::{stamp_paper, Lift, Stamp, StampLayout, CREAM, INK, MARKER},
    stamp_art::STAMP_TABS,
};
use crate::app::demo_tab::local_active_tab;

// How many window heights the section takes.
pub(super) const STAMPS_PAGES: f32 = 1.0;
const MARIGOLD: Color = Color(0.961, 0.722, 0.18, 1.0);
const MARIGOLD_WGSL: &str = concat!(include_str!("noise.wgsl"), include_str!("marigold.wgsl"));
// How deep the sheet lies in front of the ground.
const SHEET_DEPTH: f32 = -6.0;
// Stamps that never lie quite flat, so the sheet reads as loose paper.
const RESTING: [(usize, f32); 2] = [(8, 0.2), (10, 0.13)];
// A stage narrower than this takes the phone's board.
const NARROW: f32 = 700.0;

// A sheet's design, in the unit of its board: how many stamps across and
// down, a stamp's picture, the margins round it and the strip for its
// name, the title, and the sheet's margins and perforation.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Board {
    size: Size,
    columns: usize,
    rows: usize,
    art: Size,
    side: f32,
    head: f32,
    name_strip: f32,
    name: f32,
    // How far the block of stamps sits below the middle, for the title.
    drop: f32,
    title: f32,
    // How far above the stamps the title starts, and its height.
    title_box: (f32, f32),
    // The sheet's margins: beside, above and below the stamps.
    margins: (f32, f32, f32),
    hole_pitch: f32,
    hole: f32,
}

const WIDE: Board = Board {
    size: Size::new(1310.0, 720.0),
    columns: 5,
    rows: 3,
    art: Size::new(214.0, 134.0),
    side: 11.5,
    head: 10.5,
    name_strip: 26.5,
    name: 16.5,
    drop: 30.0,
    title: 56.0,
    title_box: (71.0, 66.0),
    margins: (24.0, 80.0, 22.0),
    hole_pitch: 8.5,
    hole: 2.85,
};

// Phones: three columns of smaller stamps, the title may wrap.
const PHONE: Board = Board {
    size: Size::new(390.0, 780.0),
    columns: 3,
    rows: 5,
    art: Size::new(105.0, 66.0),
    side: 6.5,
    head: 7.0,
    name_strip: 21.0,
    name: 13.0,
    drop: 34.0,
    title: 35.0,
    title_box: (80.0, 74.0),
    margins: (10.0, 88.0, 14.0),
    hole_pitch: 6.4,
    hole: 2.1,
};

// Where the sheet and its stamps lie on a stage.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SheetLayout {
    u: f32,
    columns: usize,
    rows: usize,
    stamp: StampLayout,
    // The block of stamps' top left corner.
    grid: Point,
    // The whole sheet of paper.
    sheet: Rect,
    // The title's size and the box it stands in, above the stamps.
    title: f32,
    title_box: (f32, f32),
}

impl SheetLayout {
    fn new(size: Size) -> Self {
        let board = if size.width < NARROW { PHONE } else { WIDE };
        let u = (size.height / board.size.height)
            .min(size.width / board.size.width)
            .max(0.3);
        // Stamps fall on whole pixels, so their pictures and perforations
        // keep to the pixel grid.
        let round = |value: f32| (value * u).round();
        let (art_w, art_h) = (round(board.art.width), round(board.art.height));
        let (side, head) = (round(board.side), round(board.head));
        let stamp = Size::new(art_w + 2.0 * side, art_h + head + round(board.name_strip));
        let (columns, rows) = (board.columns as f32, board.rows as f32);
        let grid = Point::new(
            ((size.width - stamp.width * columns) * 0.5).floor(),
            ((size.height - rows * stamp.height) * 0.5 + board.drop * u).round(),
        );
        let (margin, top, bottom) = (
            board.margins.0 * u,
            board.margins.1 * u,
            board.margins.2 * u,
        );
        let hole_pitch = board.hole_pitch * u;
        Self {
            u,
            columns: board.columns,
            rows: board.rows,
            stamp: StampLayout {
                size: stamp,
                art: Rect {
                    x: side,
                    y: head,
                    width: art_w,
                    height: art_h,
                },
                name_size: (board.name * u).max(10.0),
                hole: board.hole * u,
                holes: (
                    (stamp.width / hole_pitch).round() as u32,
                    (stamp.height / hole_pitch).round() as u32,
                ),
            },
            grid,
            sheet: Rect {
                x: grid.x - margin,
                y: grid.y - top,
                width: stamp.width * columns + 2.0 * margin,
                height: stamp.height * rows + top + bottom,
            },
            title: board.title * u,
            title_box: (board.title_box.0 * u, board.title_box.1 * u),
        }
    }

    fn stamp_at(&self, index: usize) -> Point {
        Point::new(
            self.grid.x + (index % self.columns) as f32 * self.stamp.size.width,
            self.grid.y + (index / self.columns) as f32 * self.stamp.size.height,
        )
    }

    // The block of stamps on the sheet's paper.
    fn grid_on_sheet(&self) -> Rect {
        Rect {
            x: self.grid.x - self.sheet.x,
            y: self.grid.y - self.sheet.y,
            width: self.stamp.size.width * self.columns as f32,
            height: self.stamp.size.height * self.rows as f32,
        }
    }
}

// The section, `top` pixels down the article that `scroll` scrolls.
#[composable(no_skip)]
pub(super) fn StampsSection(size: Size, scroll: ScrollState, top: f32) {
    let layout = SheetLayout::new(size);
    let open = local_active_tab().current();
    let stamp = layout.stamp.size;
    key((stamp.width.to_bits(), stamp.height.to_bits()), move || {
        let curls = remember(move || Rc::new(CurlMeshes::new(stamp))).with(Rc::clone);
        SceneStage(size, Some(STAGE_LIGHT), move || {
            Ground(size);
            Sheet(layout);
            for (index, tab) in STAMP_TABS.into_iter().enumerate() {
                let curls = Rc::clone(&curls);
                key(index, move || {
                    let lift = Rc::new(Lift {
                        resting: RESTING
                            .iter()
                            .find(|(at, _)| *at == index)
                            .map_or(0.0, |(_, lift)| *lift),
                        arriving: Rc::new(move || {
                            arrival(scroll, (top, size.height), (index, layout))
                        }),
                    });
                    Stamp(
                        tab,
                        (layout.stamp_at(index), layout.stamp),
                        (curls, lift),
                        open,
                    );
                });
            }
        });
    });
}

// The lift stamp `index` still has from settling onto the sheet while the
// section, `top` down a page `height` tall, comes in: from the top left to
// the bottom right, one after another.
fn arrival(
    scroll: ScrollState,
    (top, height): (f32, f32),
    (index, layout): (usize, SheetLayout),
) -> f32 {
    let entered = ((scroll.value() - top + height) / height.max(1.0)).clamp(0.0, 1.0);
    let progress = ((entered - 0.45) / 0.55).clamp(0.0, 1.0);
    let across = (index % layout.columns) as f32 / (layout.columns - 1) as f32;
    let down = (index / layout.columns) as f32 / (layout.rows - 1) as f32;
    let delay = (across + down) * 0.5 * 0.6;
    let settled = ((progress - delay) / 0.4).clamp(0.0, 1.0);
    0.5 * (1.0 - settled * settled * (3.0 - 2.0 * settled))
}

// The marigold paper the sheet lies on.
#[composable(no_skip)]
fn Ground(size: Size) {
    let plane = remember(|| Arc::new(Mesh::plane())).with(Arc::clone);
    Box(
        Modifier::empty()
            .required_size(size)
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&plane),
                transform: Mat4::scaling(Vec3::new(size.width, size.height, 1.0)),
                material: marigold(size),
                casts_shadow: false,
                solid: true,
            })
            .background(MARIGOLD),
        BoxSpec::default(),
        || {},
    );
}

fn marigold(size: Size) -> Material {
    static SHADER: OnceLock<Arc<str>> = OnceLock::new();
    let mut params = [0.0; Shaded::PARAMS];
    params[..2].copy_from_slice(&[size.width, size.height]);
    Material::Shaded(Shaded {
        shader: Arc::clone(SHADER.get_or_init(|| Arc::from(MARIGOLD_WGSL))),
        params,
    })
}

// The sheet's paper round the block of stamps, with the title on its top
// margin and the printer's marks in its margins. The stamps lie in the
// hole it leaves.
#[composable(no_skip)]
fn Sheet(layout: SheetLayout) {
    let plane = remember(|| Arc::new(Mesh::plane())).with(Arc::clone);
    let sheet = layout.sheet;
    let size = Size::new(sheet.width, sheet.height);
    let u = layout.u;
    Box(
        Modifier::empty()
            .offset(sheet.x, sheet.y)
            .required_size(size)
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&plane),
                transform: Mat4::scaling(Vec3::new(size.width, size.height, 1.0))
                    .then(&Mat4::translation(Vec3::new(0.0, 0.0, SHEET_DEPTH))),
                material: stamp_paper(size, Bend::at(0.0, size), None, 64.0 * u),
                // The sheet lies flat: it would only shadow itself.
                casts_shadow: false,
                solid: true,
            }),
        BoxSpec::default(),
        move || {
            Canvas(Modifier::empty().required_size(size), move |scope| {
                draw_sheet(scope, layout);
            });
            let marker = TextSelectionColors {
                handle_color: MARKER,
                background_color: MARKER,
            };
            CompositionLocalProvider(
                [local_text_selection_colors().provides(marker)],
                move || {
                    SelectionContainer(
                        Modifier::empty()
                            .offset(0.0, layout.grid.y - sheet.y - layout.title_box.0)
                            .required_size(Size::new(size.width, layout.title_box.1)),
                        move || {
                            let mut style = headline(layout.title, INK);
                            style.paragraph_style.text_align = TextAlign::Center;
                            Text(
                                "Some demos in this app",
                                Modifier::empty().fill_max_width(),
                                style,
                            );
                        },
                    );
                },
            );
        },
    );
}

// The sheet's paper: cream, cut away where the stamps lie, perforated
// round them, with six colour dots under the first stamp and a register
// cross in each top corner.
fn draw_sheet(scope: &mut dyn cranpose_ui::DrawScope, layout: SheetLayout) {
    let sheet = layout.sheet;
    let u = layout.u;
    let grid = layout.grid_on_sheet();
    let mut paper = Path::new();
    paper.move_to(Point::new(0.0, 0.0));
    paper.line_to(Point::new(sheet.width, 0.0));
    paper.line_to(Point::new(sheet.width, sheet.height));
    paper.line_to(Point::new(0.0, sheet.height));
    paper.close();
    let (across, down) = layout.stamp.holes;
    let holes = (across * layout.columns as u32, down * layout.rows as u32);
    perforated(
        &mut paper,
        grid,
        holes,
        layout.stamp.hole,
        Winding::CounterClockwise,
    );
    scope.draw_path(&paper, Brush::solid(CREAM), DrawStyle::Fill);
    let stamp = layout.stamp.size;
    let dots_y = grid.y + grid.height + (sheet.height - grid.y - grid.height) * 0.48;
    let colors = [
        Color(0.0, 0.36, 0.72, 1.0),
        Color(0.78, 0.02, 0.20, 1.0),
        Color(0.93, 0.60, 0.0, 1.0),
        Color(0.02, 0.02, 0.02, 1.0),
        Color(0.02, 0.10, 0.55, 1.0),
        Color(0.0, 0.33, 0.12, 1.0),
    ];
    for (index, color) in colors.into_iter().enumerate() {
        let x = grid.x + stamp.width * 0.5 - 37.5 * u + index as f32 * 15.0 * u;
        scope.draw_circle(Brush::solid(color), Point::new(x, dots_y), 4.3 * u);
    }
    let margin = grid.x;
    if margin > 17.0 * u {
        for x in [margin * 0.5, sheet.width - margin * 0.5] {
            let centre = Point::new(x, grid.y - 38.0 * u);
            let ink = Brush::solid(INK);
            let stroke = cranpose_ui::Stroke::new(u.max(1.0));
            scope.draw_circle_stroked(ink.clone(), centre, 4.6 * u, stroke);
            let arm = 8.0 * u;
            scope.draw_line(
                ink.clone(),
                Point::new(centre.x - arm, centre.y),
                Point::new(centre.x + arm, centre.y),
                stroke,
            );
            scope.draw_line(
                ink,
                Point::new(centre.x, centre.y - arm),
                Point::new(centre.x, centre.y + arm),
                stroke,
            );
        }
    }
}
