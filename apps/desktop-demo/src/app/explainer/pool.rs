// The pool section: words laid as a mosaic on the floor of a swimming pool
// and read through moving water. They are live composables: they select
// and copy, and the writing lane among them takes typing. Each typed letter
// drops into the water where the caret stands, and a pointer moving over
// the pool leaves a wake.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{Arc, OnceLock},
};

use cranpose_core::{key, remember, rememberMutableStateOf, CompositionLocalProvider};
use cranpose_foundation::text::TextFieldState;
use cranpose_ui::{
    collect_is_hovered_as_state, composable, local_text_selection_colors,
    rememberMutableInteractionSource,
    space::{Mat4, Material, Mesh, MeshSurface, Shaded, Vec3},
    text::{measure_text_with_options, AnnotatedString, TextLayoutOptions, TextUnit},
    widgets::SelectionContainer,
    Box, BoxSpec, Color, Column, ColumnSpec, Modifier, MutableInteractionSource, Point,
    PointerEventKind, PointerInputScope, Size, Text, TextSelectionColors, TextStyle,
};

use super::{
    lane::{Lane, WritingLane, FIRST_WORDS},
    lettering::{headline, strong},
    ripples::Disturbance,
    stage::SceneStage,
    swell::Swell,
    walk::unit,
    water::{PoolShape, Water},
};
use crate::app::ticking::Ticking;

// How many window heights the section takes.
pub(super) const POOL_PAGES: f32 = 1.0;
// The content's key colours; `pool.wgsl` reads the others from the lane's.
const GROUND: Color = Color::WHITE;
const INK: Color = Color::BLACK;
const SELECTION: Color = Color(1.0, 0.0, 1.0, 1.0);
const HEADLINE: &str = "You can type\nunderwater";
const WORDS: &str = "These words lie at the bottom of a pool, and they are still \
                     composables: select them, copy them. Click the line below and type. \
                     Each letter makes a ripple.";
const POOL_WGSL: &str = cranpose_ui::include_wgsl!("pool.wgsl");
const NOISE_WGSL: &str = cranpose_ui::include_wgsl!("noise.wgsl");
// How fast the lane's glow and first words follow, per second.
const EASE_RATE: f32 = 10.0;

// The floor as the water shows it this frame, and what its shader reads.
#[derive(Clone, PartialEq)]
struct FloorNow {
    mesh: Arc<Mesh>,
    params: [f32; Shaded::PARAMS],
}

// Where the words lie on the floor, in a grid of tiles.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    u: f32,
    tile: f32,
    // The side of the text column, in tiles from the stage's sides.
    side: f32,
    // The text column's top, in tiles from the stage's top.
    top: f32,
}

impl Layout {
    fn new(size: Size) -> Self {
        let u = unit(size);
        Self {
            u,
            tile: 24.0 * u,
            side: 4.0,
            top: 5.0,
        }
    }

    fn headline_style(&self) -> TextStyle {
        let mut style = headline(150.0 * self.u, INK);
        style.paragraph_style.line_height = TextUnit::Sp(6.0 * self.tile);
        style
    }

    fn words_style(&self) -> TextStyle {
        let mut style = strong(36.0 * self.u, INK);
        style.paragraph_style.line_height = TextUnit::Sp(2.0 * self.tile);
        style
    }

    fn words_width(&self, size: Size) -> f32 {
        (1080.0 * self.u).min(self.column_width(size))
    }

    fn column_width(&self, size: Size) -> f32 {
        (size.width - 2.0 * self.side * self.tile).max(1.0)
    }

    // The headline's lower edge: two lines of six tiles.
    fn headline_end(&self) -> f32 {
        (self.top + 12.0) * self.tile
    }

    // Whether `point` lies among the words: on the headline and the
    // paragraph, or in the lane under them.
    fn among_words(&self, point: Point, size: Size, lane: &Lane) -> bool {
        let left = self.side * self.tile;
        let in_words = (left..left + self.words_width(size)).contains(&point.x)
            && (self.top * self.tile..lane.top_left.y).contains(&point.y);
        let in_lane = (left..left + lane.width).contains(&point.x)
            && (lane.top_left.y..lane.top_left.y + 6.0 * self.tile).contains(&point.y);
        in_words || in_lane
    }

    // The lane under the words: a tile above the words, two below them.
    fn lane(&self, size: Size) -> Lane {
        let words = measure_text_with_options(
            &AnnotatedString::from(WORDS),
            &self.words_style(),
            TextLayoutOptions::default(),
            Some(self.words_width(size)),
        );
        let lines = words.line_count.max(1) as f32;
        let top = self.headline_end() + (1.0 + 2.0 * lines + 2.0) * self.tile;
        Lane::new(
            Point::new(self.side * self.tile, top),
            self.column_width(size),
            (self.u, self.tile),
            INK,
        )
    }

    fn shape(&self, size: Size) -> PoolShape {
        PoolShape {
            size,
            look: self.u.clamp(0.6, 1.5),
            waterline: 58.0 * self.u,
            wall_foot: 98.0 * self.u,
        }
    }
}

// The section, which plays while `moving` holds.
#[composable(no_skip)]
pub(super) fn PoolSection(size: Size, moving: bool) {
    let field = remember(|| TextFieldState::new(FIRST_WORDS)).with(|field| *field);
    // A stage of another size fills with water of its own.
    key((size.width.to_bits(), size.height.to_bits()), move || {
        PoolStage(size, moving, field);
    });
}

#[composable(no_skip)]
fn PoolStage(size: Size, moving: bool, field: TextFieldState) {
    let layout = Layout::new(size);
    let lane = remember(move || layout.lane(size)).with(Clone::clone);
    let water =
        remember(move || Rc::new(RefCell::new(Water::new(layout.shape(size))))).with(Rc::clone);
    let first = Rc::clone(&water);
    let floor = rememberMutableStateOf(move || FloorNow {
        mesh: Arc::new(first.borrow().floor_mesh()),
        params: params(&first.borrow(), layout, (0.0, 1.0)),
    });
    let hover = rememberMutableInteractionSource();
    let hovered = collect_is_hovered_as_state(&hover);
    let (ticking_water, ticking_lane) = (Rc::clone(&water), lane.clone());
    let seen = remember(|| {
        Rc::new(Cell::new((
            field.text_revision(),
            field.with_text(str::len),
        )))
    })
    .with(Rc::clone);
    let eased = remember(|| Rc::new(Cell::new((0.0f32, 1.0f32)))).with(Rc::clone);
    let greeted = remember(|| Rc::new(Cell::new(false))).with(Rc::clone);
    Ticking(moving, move |seconds| {
        let mut water = ticking_water.borrow_mut();
        if !greeted.replace(true) {
            greet(&mut water, &ticking_lane);
        }
        let (revision, length) = (field.text_revision(), field.with_text(str::len));
        let (seen_revision, seen_length) = seen.replace((revision, length));
        if revision != seen_revision {
            let kind = if length < seen_length {
                Disturbance::Dimple
            } else {
                Disturbance::Drop { strength: 1.0 }
            };
            water.disturb(ticking_lane.caret_point(field), kind, 0.0);
        }
        water.advance(seconds);
        let ease = 1.0 - (-seconds * EASE_RATE).exp();
        let (invite, words) = eased.get();
        let first_words = field.with_text(|text| f32::from(u8::from(text == FIRST_WORDS)));
        let invite_goal = f32::from(u8::from(hovered.get()));
        let now = (
            invite + (invite_goal - invite) * ease,
            words + (first_words - words) * ease,
        );
        eased.set(now);
        floor.set(FloorNow {
            mesh: Arc::new(water.floor_mesh()),
            params: params(&water, layout, now),
        });
    });
    let wake_water = Rc::clone(&water);
    SceneStage(size, None, move || {
        let (lane, wake_water) = (lane.clone(), Rc::clone(&wake_water));
        let stir_lane = lane.clone();
        Box(
            Modifier::empty()
                .required_size(size)
                .mesh_surface(move || {
                    let now = floor.get();
                    MeshSurface {
                        mesh: now.mesh,
                        transform: Mat4::scaling(Vec3::new(size.width, size.height, 1.0)),
                        material: pool_material(now.params),
                        casts_shadow: false,
                        solid: true,
                    }
                })
                .background(GROUND)
                .pointer_input((), move |scope: PointerInputScope| {
                    let (water, lane) = (Rc::clone(&wake_water), stir_lane.clone());
                    async move { stir(scope, water, (layout, size, lane)).await }
                }),
            BoxSpec::default(),
            move || FloorWords(size, layout, (lane.clone(), field, hover)),
        );
    });
}

// The words on the floor, in the content's key colours.
#[composable(no_skip)]
fn FloorWords(
    size: Size,
    layout: Layout,
    (lane, field, hover): (Lane, TextFieldState, MutableInteractionSource),
) {
    let marker = TextSelectionColors {
        handle_color: SELECTION,
        background_color: SELECTION,
    };
    CompositionLocalProvider(
        [local_text_selection_colors().provides(marker)],
        move || {
            let lane = lane.clone();
            Column(
                Modifier::empty().fill_max_size().padding_each(
                    layout.side * layout.tile,
                    layout.top * layout.tile,
                    layout.side * layout.tile,
                    0.0,
                ),
                ColumnSpec::default(),
                move || {
                    SelectionContainer(Modifier::empty(), move || {
                        Column(Modifier::empty(), ColumnSpec::default(), move || {
                            Text(HEADLINE, Modifier::empty(), layout.headline_style());
                            Box(
                                Modifier::empty().padding_each(
                                    0.0,
                                    layout.tile,
                                    0.0,
                                    2.0 * layout.tile,
                                ),
                                BoxSpec::default(),
                                move || {
                                    Text(
                                        WORDS,
                                        Modifier::empty().width(layout.words_width(size)),
                                        layout.words_style(),
                                    );
                                },
                            );
                        });
                    });
                    WritingLane(lane.clone(), field, hover);
                },
            );
        },
    );
}

// Rings run along the lane as the section first plays, as if it were
// being typed in.
fn greet(water: &mut Water, lane: &Lane) {
    let y = lane.top_left.y + 3.0 * lane.tile;
    let spacing = (lane.width / 10.0).max(lane.tile);
    for ring in 0..9 {
        let at = Point::new(lane.top_left.x + 2.0 * lane.tile + ring as f32 * spacing, y);
        water.disturb(
            at,
            Disturbance::Drop { strength: 0.5 },
            0.4 + 0.13 * ring as f32,
        );
    }
}

// Follows the pointer over the floor: a moving pointer leaves a wake; a
// press among the words drops into the water lightly and the pointer leaves
// no wake until it lifts, so they hold still to be selected, and a press on
// bare floor splashes.
async fn stir(
    scope: PointerInputScope,
    water: Rc<RefCell<Water>>,
    (layout, size, lane): (Layout, Size, Lane),
) {
    scope
        .await_pointer_event_scope(move |events| async move {
            let mut last: Option<Point> = None;
            let mut selecting = false;
            loop {
                let event = events.await_pointer_event().await;
                let at = event.position;
                match event.kind {
                    PointerEventKind::Move => {
                        if let Some(from) = last.filter(|_| !selecting) {
                            let moved = ((at.x - from.x).powi(2) + (at.y - from.y).powi(2)).sqrt();
                            if moved > 0.5 && moved < 400.0 {
                                let strength = -(2.5 + 5.0 * (moved / 40.0).min(1.0));
                                water
                                    .borrow_mut()
                                    .disturb(at, Disturbance::Wake { strength }, 0.0);
                            }
                        }
                        last = Some(at);
                    }
                    PointerEventKind::Down => {
                        selecting = layout.among_words(at, size, &lane);
                        let kind = if selecting {
                            Disturbance::Drop { strength: 0.3 }
                        } else {
                            Disturbance::Splash
                        };
                        water.borrow_mut().disturb(at, kind, 0.0);
                    }
                    PointerEventKind::Up => selecting = false,
                    PointerEventKind::Exit | PointerEventKind::Cancel => {
                        last = None;
                        selecting = false;
                    }
                    _ => {}
                }
            }
        })
        .await;
}

fn params(
    water: &Water,
    layout: Layout,
    (invite, first_words): (f32, f32),
) -> [f32; Shaded::PARAMS] {
    let shape = water.shape();
    let mut params = [0.0; Shaded::PARAMS];
    params[..10].copy_from_slice(&[
        shape.size.width,
        shape.size.height,
        water.clock(),
        shape.look,
        layout.tile,
        shape.waterline,
        shape.wall_foot,
        invite,
        first_words,
        layout.headline_end(),
    ]);
    params
}

fn pool_material(params: [f32; Shaded::PARAMS]) -> Material {
    static SHADER: OnceLock<Arc<str>> = OnceLock::new();
    let shader =
        SHADER.get_or_init(|| Arc::from(format!("{NOISE_WGSL}{}{POOL_WGSL}", Swell::new().wgsl())));
    Material::Shaded(Shaded {
        shader: Arc::clone(shader),
        params,
    })
}
