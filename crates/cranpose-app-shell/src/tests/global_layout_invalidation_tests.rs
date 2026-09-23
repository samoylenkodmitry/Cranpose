use cranpose_ui_layout::MeasureScope;

use super::*;

struct FontSizedTextMeasurer;

impl FontSizedTextMeasurer {
    fn width(text: &cranpose_ui::text::AnnotatedString, style: &TextStyle) -> f32 {
        text.text.chars().count() as f32 * style.resolve_font_size(14.0)
    }
}

impl cranpose_ui::TextMeasurer for FontSizedTextMeasurer {
    fn measure(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &TextStyle,
    ) -> cranpose_ui::TextMetrics {
        let size = style.resolve_font_size(14.0);
        cranpose_ui::TextMetrics {
            width: Self::width(text, style),
            height: size,
            line_height: size,
            line_count: 1,
        }
    }

    fn get_offset_for_position(
        &self,
        _text: &cranpose_ui::text::AnnotatedString,
        _style: &TextStyle,
        _x: f32,
        _y: f32,
    ) -> usize {
        0
    }

    fn get_cursor_x_for_offset(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &TextStyle,
        _offset: usize,
    ) -> f32 {
        Self::width(text, style)
    }

    fn layout(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &TextStyle,
    ) -> cranpose_ui::TextLayoutResult {
        let size = style.resolve_font_size(14.0);
        cranpose_ui::TextLayoutResult::monospaced(&text.text, size, size)
    }
}

const GRID_WIDTH_PER_UNIT: f32 = 10.0;

const SCALED_LABEL: &str = "Scaled label";
const NEIGHBOUR_LABEL: &str = "Neighbour";

#[composable]
#[allow(non_snake_case)]
fn LabelsBesideAGridWideBox(grid_box: Rc<Cell<Option<NodeId>>>) {
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Text(SCALED_LABEL, Modifier::empty(), TextStyle::default());
        grid_box.set(Some(cranpose_ui::SubcomposeLayout(
            Modifier::empty(),
            |scope, _constraints| {
                let width = scope.density() * scope.font_scale() * GRID_WIDTH_PER_UNIT;
                scope.layout(width, 1.0, [])
            },
        )));
        Text(NEIGHBOUR_LABEL, Modifier::empty(), TextStyle::default());
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
        shell.app_context().set_text_measurer(FontSizedTextMeasurer);
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
            Rc::clone(self.shell.app_context()).enter(|| cranpose_ui::schedule_layout_repass(node));
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
        fn find(node: &cranpose_ui::LayoutBox, id: NodeId) -> Option<f32> {
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

    Rc::clone(fixture.shell.app_context()).enter(|| cranpose_ui::set_font_scale(2.0));
    fixture.neighbour_does(Neighbour::Repassing);

    assert_eq!(
        fixture.scaled_label_size().0,
        width * 2.0,
        "the scoped repass downgraded the global invalidation to a subtree pass"
    );
}
