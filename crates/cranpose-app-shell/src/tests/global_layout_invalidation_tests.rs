use cranpose_ui_layout::MeasureScope;

use super::*;

const GRID_WIDTH_PER_UNIT: f32 = 10.0;

const SCALED_LABEL: &str = "Scaled label";
const NEIGHBOUR_LABEL: &str = "Neighbour";

// Labels at 10 sp, where a character of Cranpose Test Mono is a whole 6
// pixels wide and a line 10 tall, so scaled sizes stay whole.
fn label_style() -> TextStyle {
    TextStyle {
        span_style: cranpose_ui::text::SpanStyle {
            font_size: cranpose_ui::text::TextUnit::Sp(10.0),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[composable]
fn LabelsBesideAGridWideBox(grid_box: Rc<Cell<Option<NodeId>>>) {
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Text(SCALED_LABEL, Modifier::empty(), label_style());
        grid_box.set(Some(cranpose_ui::SubcomposeLayout(
            Modifier::empty(),
            |scope, _constraints| {
                let width = scope.density() * scope.font_scale() * GRID_WIDTH_PER_UNIT;
                scope.layout(width, 1.0, [])
            },
        )));
        Text(NEIGHBOUR_LABEL, Modifier::empty(), label_style());
    });
}

struct Fixture {
    shell: AppShell<TestRenderer>,
    grid_box: NodeId,
    neighbour: NodeId,
}

impl Fixture {
    fn new() -> Self {
        let grid_box = Rc::new(Cell::new(None));
        let grid_box_in_content = Rc::clone(&grid_box);
        let mut shell = AppShell::new(
            TestRenderer::default(),
            location_key(file!(), line!(), column!()),
            move || LabelsBesideAGridWideBox(Rc::clone(&grid_box_in_content)),
        );
        shell.update();
        let neighbour = {
            let tree = shell.layout_tree().expect("layout tree available");
            find_layout_box_with_text(tree.root(), NEIGHBOUR_LABEL)
                .expect("neighbour label laid out")
                .node_id
        };
        Self {
            shell,
            grid_box: grid_box.get().expect("grid box composed"),
            neighbour,
        }
    }

    fn neighbour_does(&self, neighbour: Neighbour) {
        let node = self.neighbour;
        if let Neighbour::Repassing = neighbour {
            self.shell
                .app_context()
                .enter(|| cranpose_ui::schedule_layout_repass(node));
        }
    }

    fn scaled_label_size(&mut self) -> (f32, f32) {
        self.shell.update();
        let tree = self.shell.layout_tree().expect("layout tree available");
        let found =
            find_layout_box_with_text(tree.root(), SCALED_LABEL).expect("scaled label laid out");
        (found.rect.width, found.rect.height)
    }

    fn grid_box_width(&mut self) -> f32 {
        fn find(node: &LayoutBox, id: NodeId) -> Option<f32> {
            if node.node_id == id {
                return Some(node.rect.width);
            }
            node.children.iter().find_map(|child| find(child, id))
        }
        self.shell.update();
        let tree = self.shell.layout_tree().expect("layout tree available");
        find(tree.root(), self.grid_box).expect("grid box laid out")
    }
}

#[derive(Clone, Copy)]
enum Neighbour {
    Idle,
    Repassing,
}

fn assert_font_scale_resizes_text(neighbour: Neighbour) {
    let _guard = test_guard();
    let mut fixture = Fixture::new();
    let (width, height) = fixture.scaled_label_size();

    fixture.shell.set_font_scale(1.5);
    fixture.neighbour_does(neighbour);

    assert_eq!(
        fixture.scaled_label_size(),
        (width * 1.5, height * 1.5),
        "text keeps the size it measured at the old font scale and draws past its box"
    );
    assert_eq!(
        fixture.grid_box_width(),
        1.5 * GRID_WIDTH_PER_UNIT,
        "a node composed before the font scale changed measures with the old one"
    );
}

fn assert_density_remeasures_the_grid_box(neighbour: Neighbour) {
    let _guard = test_guard();
    let mut fixture = Fixture::new();
    assert_eq!(fixture.grid_box_width(), GRID_WIDTH_PER_UNIT);

    fixture.shell.set_density(2.0);
    fixture.neighbour_does(neighbour);

    assert_eq!(
        fixture.grid_box_width(),
        2.0 * GRID_WIDTH_PER_UNIT,
        "a node measured at the old density keeps that measurement"
    );
}

#[test]
fn a_font_scale_change_remeasures_text_that_did_not_change() {
    assert_font_scale_resizes_text(Neighbour::Idle);
}

#[test]
fn a_font_scale_change_remeasures_text_while_a_scoped_repass_is_pending() {
    assert_font_scale_resizes_text(Neighbour::Repassing);
}

#[test]
fn a_density_change_remeasures_a_node_that_did_not_change() {
    assert_density_remeasures_the_grid_box(Neighbour::Idle);
}

#[test]
fn a_density_change_remeasures_layout_while_a_scoped_repass_is_pending() {
    assert_density_remeasures_the_grid_box(Neighbour::Repassing);
}

#[test]
fn a_global_layout_invalidation_survives_a_scoped_repass_in_the_same_frame() {
    let _guard = test_guard();
    let mut fixture = Fixture::new();
    let (width, _) = fixture.scaled_label_size();

    fixture
        .shell
        .app_context()
        .enter(|| cranpose_ui::set_font_scale(2.0));
    fixture.neighbour_does(Neighbour::Repassing);

    assert_eq!(
        fixture.scaled_label_size().0,
        width * 2.0,
        "the scoped repass downgraded the global invalidation to a subtree pass"
    );
}

thread_local! {
    /// The safe area's top edge, which the root provides as a platform's
    /// environment does: only a root render reads a new one.
    static PADDING_TOP: Cell<f32> = const { Cell::new(5.0) };
    static TICKER: Cell<Option<MutableState<u32>>> = const { Cell::new(None) };
}

const PADDED_ROW: Color = Color(0.9, 0.1, 0.2, 1.0);

fn safe_area_root() {
    let insets =
        cranpose_ui::EdgeInsets::from_components(0.0, PADDING_TOP.with(Cell::get), 0.0, 0.0);
    CompositionLocalProvider(
        [cranpose_ui::local_safe_area_insets().provides(insets)],
        PaddedColumnBesideATicker,
    );
}

#[composable]
fn PaddedColumnBesideATicker() {
    let top = cranpose_ui::local_safe_area_insets().current().top;
    Column(
        Modifier::empty()
            .fill_max_size()
            .padding_each(0.0, top, 0.0, 0.0),
        ColumnSpec::default(),
        || {
            Box(
                Modifier::empty()
                    .size_points(60.0, 10.0)
                    .background(PADDED_ROW),
                BoxSpec::default(),
                || {},
            );
            Ticker();
        },
    );
}

/// Text that changes in its own scope, as an animation's does.
#[composable]
fn Ticker() {
    let ticks = rememberMutableStateOf(|| 0_u32);
    TICKER.with(|slot| slot.set(Some(ticks)));
    Text(
        format!("tick {}", ticks.get()),
        Modifier::empty(),
        TextStyle::default(),
    );
}

#[test]
fn a_root_render_that_moves_content_reaches_the_scene_while_a_scope_redraws() {
    let _guard = test_guard();
    PADDING_TOP.with(|top| top.set(5.0));
    let mut shell = AppShell::new_with_size(
        ScopedUpdateCountingRenderer::new(
            Rc::new(Cell::new(0)),
            Rc::new(Cell::new(0)),
            Rc::new(RefCell::new(Vec::new())),
        ),
        location_key(file!(), line!(), column!()),
        safe_area_root,
        (100, 100),
        (100.0, 100.0),
    );
    shell.update();
    shell.update();

    // The safe area grows through a root render, and the ticker changes in
    // the same frame.
    PADDING_TOP.with(|top| top.set(40.0));
    shell.request_root_render();
    let ticks = TICKER.with(Cell::get).expect("the ticker composed");
    ticks.set_value(1);
    shell.update();

    let (retained, expected) = retained_and_fresh_painted_rects(&mut shell);
    assert_eq!(expected.first().map(|(rect, _)| rect.y), Some(40.0));
    assert_eq!(
        retained, expected,
        "the scene drew the column's children where the old padding put them"
    );
}
