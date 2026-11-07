// A child of a parent turned by its graphics layer, which draws through a
// surface of its own, is where it shows in the window for every reader of
// the scene: a press reaches it, the window bounds a screen reader takes
// follow the turn, and a change of the state its draws read redraws it, also after
// the shell dropped the draw observations of nodes no scene holds.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key, rememberMutableStateOf};
use cranpose_macros::composable;
use cranpose_render_common::{RenderScene, Renderer};
use cranpose_render_pixels::PixelsRenderer;
use cranpose_ui::{Box, BoxSpec, Modifier, Text, TextStyle};
use cranpose_ui_graphics::{Brush, Color};

#[path = "support/turned_parent.rs"]
mod turned_parent;

use turned_parent::turned_parent;

const WIDTH: u32 = 200;
const HEIGHT: u32 = 160;
// More frames than the shell redraws between two prunes of its draw
// observations.
const PRUNED_AFTER: u32 = 70;

type State = Rc<RefCell<Option<MutableState<u32>>>>;

// The states a frame changes: the colour of the child's own background, the
// colour its draws read, and a count beside the parent that redraws content
// each frame.
#[derive(Clone, Default, PartialEq)]
struct States {
    shade: State,
    drawn: State,
    count: State,
}

impl States {
    fn set(state: &State, value: u32) {
        let state = (*state.borrow()).expect("the composition remembered the state");
        state.set(value);
    }
}

fn red(value: u32) -> Color {
    Color::from_rgb_u8((value % 4) as u8 * 60, 120, 40)
}

// A parent turned by 30 degrees over a child of 40 by 20 at (10, 10) in it
// that counts its clicks into `clicks` and draws, over its background, a
// rect in the colour of the state `drawn`, read as it draws.
#[composable]
fn TurnedButton(clicks: Rc<Cell<u32>>, states: States) {
    let shade = rememberMutableStateOf(|| 0u32);
    let drawn = rememberMutableStateOf(|| 0u32);
    let count = rememberMutableStateOf(|| 0u32);
    *states.shade.borrow_mut() = Some(shade);
    *states.drawn.borrow_mut() = Some(drawn);
    *states.count.borrow_mut() = Some(count);
    Count(count);
    Box(
        turned_parent(80.0, || 30.0),
        BoxSpec::default(),
        move || {
            let clicks = Rc::clone(&clicks);
            Box(
                Modifier::empty()
                    .offset(10.0, 10.0)
                    .width(40.0)
                    .height(20.0)
                    .background(red(shade.value()))
                    .draw_behind(move |scope| {
                        scope.draw_rect(Brush::solid(red(drawn.value() + 1)));
                    })
                    .clickable(move |_| clicks.set(clicks.get() + 1)),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

// The count, in a box of its own size: a new count changes no other node.
#[composable]
fn Count(count: MutableState<u32>) {
    Box(
        Modifier::empty().width(40.0).height(20.0),
        BoxSpec::default(),
        move || {
            Text(
                format!("{}", count.value()),
                Modifier::empty(),
                TextStyle::default(),
            );
        },
    );
}

// Where the point `(x, y)` of the parent's own space shows in the window.
fn in_window(x: f32, y: f32) -> (f32, f32) {
    let (sin, cos) = 30f32.to_radians().sin_cos();
    let (dx, dy) = (x - 40.0, y - 30.0);
    (120.0 + dx * cos - dy * sin, 80.0 + dx * sin + dy * cos)
}

struct Turned {
    shell: AppShell<PixelsRenderer>,
    clicks: Rc<Cell<u32>>,
    states: States,
    image: Vec<u8>,
}

impl Turned {
    fn new() -> Self {
        let clicks = Rc::new(Cell::new(0));
        let states = States::default();
        let content = (Rc::clone(&clicks), states.clone());
        let shell = AppShell::new_with_size_and_density(
            PixelsRenderer::new(),
            location_key(file!(), line!(), column!()),
            move || TurnedButton(Rc::clone(&content.0), content.1.clone()),
            (WIDTH, HEIGHT),
            (WIDTH as f32, HEIGHT as f32),
            1.0,
        );
        let mut turned = Self {
            shell,
            clicks,
            states,
            image: vec![0; (WIDTH * HEIGHT * 4) as usize],
        };
        turned.frame();
        turned
    }

    fn frame(&mut self) {
        self.shell.update();
        let image = &mut self.image;
        self.shell
            .primary()
            .with_renderer(|renderer| renderer.draw(image, WIDTH, HEIGHT));
    }

    fn click(&mut self, (x, y): (f32, f32)) {
        self.shell.set_cursor(x, y);
        self.shell.pointer_pressed();
        self.shell.pointer_released();
        self.shell.update();
    }

    fn pixel(&self, (x, y): (usize, usize)) -> [u8; 4] {
        let at = (y * WIDTH as usize + x) * 4;
        self.image[at..at + 4].try_into().expect("RGBA pixel")
    }
}

#[test]
fn a_press_on_a_child_of_a_turned_parent_reaches_the_child() {
    let mut turned = Turned::new();
    // The child's center, and a point of the parent beside the child.
    let (child, beside) = (in_window(30.0, 20.0), in_window(65.0, 45.0));
    turned.click(child);
    assert_eq!(
        turned.clicks.get(),
        1,
        "the first frame's press reaches the child"
    );
    turned.click(beside);
    assert_eq!(turned.clicks.get(), 1, "a press beside the child misses it");
    for step in 1..4 {
        States::set(&turned.states.shade, step);
        turned.frame();
        turned.click(child);
        assert_eq!(
            turned.clicks.get(),
            1 + step,
            "step {step}: the press reaches the child after its background changed"
        );
    }
}

#[test]
fn a_child_of_a_turned_parent_gives_a_reader_its_turned_bounds() {
    let mut turned = Turned::new();
    let corners =
        [(10.0, 10.0), (50.0, 10.0), (10.0, 30.0), (50.0, 30.0)].map(|(x, y)| in_window(x, y));
    let (left, top) = corners
        .iter()
        .fold((f32::MAX, f32::MAX), |(l, t), &(x, y)| (l.min(x), t.min(y)));
    let (right, bottom) = corners
        .iter()
        .fold((f32::MIN, f32::MIN), |(r, b), &(x, y)| (r.max(x), b.max(y)));
    let expected = [left, top, right - left, bottom - top];
    let mut found = Vec::new();
    turned.shell.primary().with_renderer(|renderer| {
        renderer.scene().node_window_bounds(&mut |_, rect| {
            found.push([rect.x, rect.y, rect.width, rect.height]);
        });
    });
    assert!(
        found.iter().any(|rect| rect
            .iter()
            .zip(expected)
            .all(|(got, want)| (got - want).abs() < 0.01)),
        "no node shows at the turned child's bounds {expected:?}: {found:?}"
    );
}

#[test]
fn a_child_of_a_turned_parent_redraws_its_draw_state_after_a_prune() {
    let mut turned = Turned::new();
    // The pixel at the child's center, which `in_window(30.0, 20.0)` puts at
    // (116.3, 66.3).
    let child = (116, 66);
    for count in 1..=PRUNED_AFTER {
        States::set(&turned.states.count, count);
        turned.frame();
    }
    let before = turned.pixel(child);
    States::set(&turned.states.drawn, 1);
    turned.frame();
    assert_ne!(
        turned.pixel(child),
        before,
        "the child draws the state its draws read after the observations were pruned"
    );
}
