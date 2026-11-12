use std::{cell::RefCell, rc::Rc};

use cranpose_core::{MutableState, NodeId};
use cranpose_render_common::{RenderScene, graph_scene::Scene, scene_store::ItemRef};
use cranpose_ui::{BoxSpec, Modifier};
use cranpose_ui_graphics::{Color, Size};

use crate::scene_probe::{initial_graph, update_scene};

const VIEWPORT: Size = Size {
    width: 120.0,
    height: 120.0,
};

// A box that holds a square while `shown` says so.
fn toggled_square(shown: Rc<RefCell<Option<MutableState<bool>>>>) -> cranpose_ui::TestComposition {
    cranpose_ui::run_test_composition(move || {
        let state = cranpose_core::rememberMutableStateOf(|| false);
        *shown.borrow_mut() = Some(state);
        cranpose_ui::Box(
            Modifier::empty().size(Size::new(100.0, 100.0)),
            BoxSpec::default(),
            move || {
                if state.get() {
                    cranpose_ui::Box(
                        Modifier::empty()
                            .size(Size::new(40.0, 40.0))
                            .background(Color::RED),
                        BoxSpec::default(),
                        || {},
                    );
                }
            },
        );
    })
}

// The node of the first record in the root record of `scene`.
fn first_child_node(scene: &Scene) -> Option<NodeId> {
    scene.root_layer()?.items().find_map(|item| match item {
        ItemRef::Layer(child) | ItemRef::Leaf(child) => child.node_id(),
        ItemRef::Draw(_) | ItemRef::Text(_) | ItemRef::Run(_) | ItemRef::Shape(_) => None,
    })
}

#[test]
fn a_shown_node_is_drawn_within_its_parent() {
    let shown = Rc::default();
    let mut composition = toggled_square(Rc::clone(&shown));
    let root = composition.root().expect("a root");
    let mut scene = initial_graph(&mut composition, root, VIEWPORT);
    let state = shown.borrow().expect("the state");

    state.set(true);
    update_scene(&mut composition, root, VIEWPORT, &mut scene);

    let square = first_child_node(&scene).expect("the square draws");
    assert!(scene.draws(square), "the square draws");
    assert_eq!(scene.drawn_parent(square), Some(root));
    assert_eq!(scene.drawn_parent(root), None, "the root draws within none");
}

#[test]
fn a_node_shown_and_hidden_again_between_reads_reads_as_left() {
    let shown = Rc::default();
    let mut composition = toggled_square(Rc::clone(&shown));
    let root = composition.root().expect("a root");
    let mut scene = initial_graph(&mut composition, root, VIEWPORT);
    let mut left = Vec::new();
    scene.read_left_nodes(&mut left);
    left.clear();
    let state = shown.borrow().expect("the state");

    state.set(true);
    update_scene(&mut composition, root, VIEWPORT, &mut scene);
    let square = first_child_node(&scene).expect("the square draws");
    state.set(false);
    update_scene(&mut composition, root, VIEWPORT, &mut scene);

    assert!(scene.read_left_nodes(&mut left), "the scene can tell");
    assert!(left.contains(&square), "the square left: {left:?}");
    assert!(!scene.draws(square), "the square no longer draws");
    assert!(scene.draws(root), "the box still draws");
}
