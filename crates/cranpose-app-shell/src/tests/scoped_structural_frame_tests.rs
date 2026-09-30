//! Frames whose layout pass inserts, moves or unplaces nodes ride the scoped
//! scene update and must draw exactly what a whole-scene rebuild draws.

use cranpose_core::key;
use cranpose_render_common::{
    graph::{LayerNode, RenderNode},
    scene_builder::build_graph_from_applier,
};
use cranpose_ui::Layout;
use cranpose_ui_layout::{
    Constraints, Measurable, MeasurePolicy, MeasureResult, MeasureScope, Placement,
};

use super::*;

#[path = "liquid_lens_frame_tests.rs"]
mod liquid_lens_frame_tests;

const FIRST_TRADE: u64 = 100;
const TAPE_ROWS: u64 = 4;
const ROW_HEIGHT: f32 = 18.0;

thread_local! {
    static TAPE_HEAD: RefCell<Option<MutableState<u64>>> = const { RefCell::new(None) };
    static PEEK_SHOWN: RefCell<Option<MutableState<bool>>> = const { RefCell::new(None) };
}

/// A time-and-sales tape: a keyed column with the newest trade on top, so
/// every new trade inserts a row at the top and pushes the others down, and
/// a footer below it that the growing column pushes down too. Nothing
/// structural names the footer: only the layout pass that moved it does.
#[composable]
fn TradeTape() {
    let head = rememberMutableStateOf(|| FIRST_TRADE + TAPE_ROWS);
    TAPE_HEAD.with(|slot| *slot.borrow_mut() = Some(head));
    let newest = head.get();
    Column(
        Modifier::empty().fill_max_size().padding(8.0),
        ColumnSpec::default(),
        move || {
            Column(
                Modifier::empty().fill_max_width(),
                ColumnSpec::default(),
                move || {
                    for trade in (FIRST_TRADE..=newest).rev() {
                        key(trade, || TradeRow(trade));
                    }
                },
            );
            Box(
                Modifier::empty()
                    .size_points(60.0, 6.0)
                    .background(Color(0.5, 0.5, 0.5, 1.0)),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

#[composable]
fn TradeRow(trade: u64) {
    Row(
        Modifier::empty().fill_max_width().height(ROW_HEIGHT),
        RowSpec::default(),
        move || {
            Text(
                format!("trade {trade}"),
                Modifier::empty(),
                TextStyle::default(),
            );
            Box(
                Modifier::empty()
                    .size_points(2.0, 12.0)
                    .background(Color(0.2, 0.8, 0.3, 1.0)),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

/// Places its first child always and its second only while `shown`, at the
/// same spot every time, in a box of fixed size. Like an overflow row it
/// measures the child it hides against the space left, a little narrower,
/// so the hidden child is measured again and left unplaced: toggling it
/// changes whether a node is placed and nothing's size or position.
#[derive(Clone, Copy, PartialEq)]
struct PeekPolicy {
    shown: bool,
}

impl MeasurePolicy for PeekPolicy {
    fn measure(
        &self,
        _scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let mut placements = Vec::with_capacity(measurables.len());
        for (index, measurable) in measurables.iter().enumerate() {
            let mut child = constraints.loosen();
            if !self.shown {
                child.max_width -= 1.0;
            }
            let placeable = measurable.measure(child);
            if index == 0 || self.shown {
                placements.push(Placement::new(
                    placeable.node_id(),
                    0.0,
                    index as f32 * 20.0,
                    0,
                ));
            }
        }
        MeasureResult::new(
            Size {
                width: 100.0,
                height: 40.0,
            },
            placements,
        )
    }

    fn min_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
        100.0
    }

    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        self.min_intrinsic_width(measurables, height)
    }

    fn min_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], _width: f32) -> f32 {
        40.0
    }

    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        self.min_intrinsic_height(measurables, width)
    }
}

/// The peeking box above a bar whose height follows the same state, so the
/// frame that hides or shows the child also resizes the bar: the frame names
/// the bar and rides the scoped update, where nothing else would drop or
/// restore the child's layer.
#[composable]
fn PeekBox() {
    let shown = rememberMutableStateOf(|| true);
    PEEK_SHOWN.with(|slot| *slot.borrow_mut() = Some(shown));
    let visible = shown.get();
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            Layout(Modifier::empty(), PeekPolicy { shown: visible }, || {
                Text(
                    "always".to_string(),
                    Modifier::empty(),
                    TextStyle::default(),
                );
                Box(
                    Modifier::empty()
                        .size_points(30.0, 10.0)
                        .background(Color(0.1, 0.2, 0.9, 1.0)),
                    BoxSpec::default(),
                    || {},
                );
            });
            Box(
                Modifier::empty()
                    .size_points(80.0, if visible { 6.0 } else { 8.0 })
                    .background(Color(0.5, 0.5, 0.5, 1.0)),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

/// Every layer's geometry and every primitive it draws, in draw order, with
/// window positions carried by the layers' transforms and offsets: what a
/// frame puts on screen, and nothing a scoped update may legitimately leave
/// different from a rebuild (cache hashes, recording identities).
fn scene_picture(layer: &LayerNode, depth: usize, out: &mut Vec<String>) {
    out.push(format!(
        "{depth} layer {:?} wraps {:?} bounds {:?} node {:?} transform {:?} content {:?} \
         origin {:?} clip {} shadow {:?} graphics {:?} translated {} {:?}",
        layer.node_id,
        layer.wraps,
        layer.local_bounds,
        layer.node_bounds,
        layer.transform_to_parent,
        layer.content_offset,
        layer.origin_in_parent,
        layer.clip_to_bounds,
        layer.shadow_clip,
        layer.graphics_layer,
        layer.translated_content_context,
        layer.translated_content_offset,
    ));
    for child in &layer.children {
        match child {
            RenderNode::Layer(child) => scene_picture(child, depth + 1, out),
            RenderNode::Primitive(entry) => out.push(format!("{depth} primitive {entry:?}")),
            RenderNode::DrawRun(run) => {
                for primitive in run.primitives() {
                    out.push(format!("{depth} run {:?} {primitive:?}", run.phase));
                }
            }
        }
    }
}

fn drawn_picture(shell: &AppShell<ScopedUpdateCountingRenderer>) -> Vec<String> {
    let graph = shell.surfaces[0]
        .renderer
        .scene()
        .graph
        .as_ref()
        .expect("the frame should have built a scene");
    let mut picture = Vec::new();
    scene_picture(&graph.root, 0, &mut picture);
    picture
}

fn rebuilt_picture(shell: &mut AppShell<ScopedUpdateCountingRenderer>) -> Vec<String> {
    let root = shell.app.composition.root().expect("composition root");
    let app_context = Rc::clone(&shell.app.app_context);
    let graph = app_context
        .enter(|| build_graph_from_applier(&shell.app.composition.applier_mut(), root, 1.0))
        .expect("a whole-scene rebuild should lower the root");
    let mut picture = Vec::new();
    scene_picture(&graph.root, 0, &mut picture);
    picture
}

/// The scene the frame left behind is the scene a whole rebuild of the same
/// tree produces: positions and draw primitives alike.
fn assert_scene_matches_rebuild(shell: &mut AppShell<ScopedUpdateCountingRenderer>, frame: &str) {
    let drawn = drawn_picture(shell);
    let rebuilt = rebuilt_picture(shell);
    if let Some(line) = drawn.iter().zip(&rebuilt).position(|(a, b)| a != b) {
        let (drawn, rebuilt) = (&drawn[line], &rebuilt[line]);
        let at = drawn
            .bytes()
            .zip(rebuilt.bytes())
            .position(|(a, b)| a != b)
            .unwrap_or(0);
        let near = |text: &str| {
            text.get(at.saturating_sub(160)..(at + 160).min(text.len()))
                .unwrap_or(text)
                .to_string()
        };
        panic!(
            "{frame}: the scoped scene differs from a rebuild at line {line}, byte {at}:\n  drawn:   {}\n  rebuilt: {}",
            near(drawn),
            near(rebuilt)
        );
    }
    assert_eq!(
        drawn.len(),
        rebuilt.len(),
        "{frame}: the scoped scene and a rebuild draw a different number of things"
    );
}

fn drawn_texts(shell: &AppShell<ScopedUpdateCountingRenderer>) -> Vec<String> {
    super::graph_scene_text_values(shell.surfaces[0].renderer.scene())
}

struct CountedShell {
    shell: AppShell<ScopedUpdateCountingRenderer>,
    rebuilds: Rc<Cell<usize>>,
    updates: Rc<Cell<usize>>,
}

impl CountedShell {
    fn settled(root_key: cranpose_core::Key, content: impl FnMut() + 'static) -> Self {
        let rebuilds = Rc::new(Cell::new(0));
        let updates = Rc::new(Cell::new(0));
        let mut shell = AppShell::new(
            ScopedUpdateCountingRenderer::new(
                Rc::clone(&rebuilds),
                Rc::clone(&updates),
                Rc::new(RefCell::new(Vec::new())),
            ),
            root_key,
            content,
        );
        shell.set_buffer_size(320, 240);
        shell.set_viewport(320.0, 240.0);
        shell.update();
        Self {
            shell,
            rebuilds,
            updates,
        }
    }

    /// Runs one frame and returns how many whole-scene rebuilds and scoped
    /// updates it took.
    fn frame(&mut self) -> (usize, usize) {
        self.rebuilds.set(0);
        self.updates.set(0);
        self.shell.update();
        (self.rebuilds.get(), self.updates.get())
    }
}

#[test]
fn rows_inserted_at_the_top_of_a_keyed_column_ride_the_scoped_update() {
    let _guard = test_guard();
    TAPE_HEAD.with(|slot| slot.borrow_mut().take());
    let mut tape = CountedShell::settled(location_key(file!(), line!(), column!()), TradeTape);
    let head = TAPE_HEAD
        .with(|slot| *slot.borrow())
        .expect("the tape exposes its newest trade");
    for _ in 0..3 {
        let newest = head.get_non_reactive() + 1;
        head.set_value(newest);
        let (rebuilds, updates) = tape.frame();
        assert_eq!(
            (rebuilds, updates),
            (0, 1),
            "a trade inserted at the top must reach the scoped update, not a rebuild"
        );
        let texts = drawn_texts(&tape.shell);
        assert_eq!(
            texts.first().map(String::as_str),
            Some(format!("trade {newest}").as_str()),
            "the new trade must draw first: {texts:?}"
        );
        assert_scene_matches_rebuild(&mut tape.shell, "keyed insertion");
    }
}

#[test]
fn an_unchanged_layout_pass_preserves_the_scene_without_recording_it() {
    let _guard = test_guard();
    let mut host = CountedShell::settled(location_key(file!(), line!(), column!()), TradeTape);
    host.frame();
    let picture = drawn_picture(&host.shell);
    host.shell.app.request_layout_pass();
    assert_eq!(host.frame(), (0, 0));
    assert_eq!(drawn_picture(&host.shell), picture);
    assert_scene_matches_rebuild(&mut host.shell, "unchanged layout");
}

#[test]
fn a_node_its_parent_stops_placing_leaves_the_scene_and_returns_where_it_was() {
    let _guard = test_guard();
    PEEK_SHOWN.with(|slot| slot.borrow_mut().take());
    let mut peek = CountedShell::settled(location_key(file!(), line!(), column!()), PeekBox);
    let shown = PEEK_SHOWN
        .with(|slot| *slot.borrow())
        .expect("the box exposes whether it shows its second child");
    let settled = drawn_picture(&peek.shell);
    for show in [false, true, false, true] {
        shown.set_value(show);
        peek.frame();
        assert_scene_matches_rebuild(
            &mut peek.shell,
            if show {
                "a child placed again at its old spot"
            } else {
                "a child no longer placed"
            },
        );
        let picture = drawn_picture(&peek.shell);
        if show {
            assert_eq!(
                picture, settled,
                "the child must come back exactly as it was"
            );
        } else {
            assert!(
                picture.len() < settled.len(),
                "the unplaced child must leave the scene"
            );
        }
    }
}
