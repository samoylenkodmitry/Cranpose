use std::{cell::RefCell, collections::VecDeque, rc::Rc, sync::Arc};

use cranpose_core::MutableState;
use cranpose_render_common::{
    graph_scene::Scene,
    scene_store::{ItemRef, LayerRef},
};
use cranpose_ui::{BoxSpec, Modifier, TestComposition};
use cranpose_ui_graphics::{Color, ShapeRecorder, Size};

use crate::scene_probe::{fresh_graph, initial_graph, solid_rects, update_scene};

// Boxes that stay for the whole run. A pass recolors one of them in turn,
// which records its background anew, so the backgrounds that stay long are
// spread over the shapes of many passes.
const STAYING: u32 = 192;
// The boxes a pass adds, each of which stays this many passes: more than a
// chunk of shapes takes in before the boxes go.
const ADDED_A_PASS: u32 = 128;
const PASSES_A_BOX_STAYS: u32 = 4;
const PASSES: u32 = 100;
// The pass after which the store's heap holds still: the chunks may first
// take a few chunks of room past their live shapes. Then the bytes by which
// the room of its hash tables and scratch lists shifts from pass to pass:
// far fewer than the 30 KB of a full chunk of shapes.
const SETTLED: usize = 50;
const SLACK: usize = 4096;

fn tint(index: u32, of: u32) -> f32 {
    (index % of) as f32 / of as f32
}

// The staying boxes, then the boxes of the passes `pass` reads, each under
// its own key.
fn churning_boxes(pass: Rc<RefCell<Option<MutableState<u32>>>>) -> TestComposition {
    cranpose_ui::run_test_composition(move || {
        let state = cranpose_core::rememberMutableStateOf(|| 0u32);
        *pass.borrow_mut() = Some(state);
        cranpose_ui::Box(
            Modifier::empty().size(Size::new(200.0, 200.0)),
            BoxSpec::default(),
            move || {
                let pass = state.get();
                for index in 0..STAYING {
                    let recolors = (pass + index) / STAYING;
                    cranpose_ui::Box(
                        Modifier::empty()
                            .size(Size::new(6.0, 6.0))
                            .background(Color(tint(index, STAYING), tint(recolors, 2), 0.5, 1.0)),
                        BoxSpec::default(),
                        || {},
                    );
                }
                let first = pass * ADDED_A_PASS;
                for id in first..first + ADDED_A_PASS * PASSES_A_BOX_STAYS {
                    cranpose_core::key(id, || {
                        cranpose_ui::Box(
                            Modifier::empty()
                                .size(Size::new(4.0, 4.0))
                                .background(Color(0.25, tint(id, 251), tint(id, 13), 1.0)),
                            BoxSpec::default(),
                            || {},
                        );
                    });
                }
            },
        );
    })
}

// The recorder of every shape item `layer` and the records beneath it draw,
// as a frame packet in flight leases them.
fn lease_shapes(layer: LayerRef<'_>, leases: &mut Vec<Arc<ShapeRecorder>>) {
    for item in layer.items() {
        match item {
            ItemRef::Shape(shape) => {
                if !leases
                    .iter()
                    .any(|held| Arc::ptr_eq(held, shape.recorder()))
                {
                    leases.push(Arc::clone(shape.recorder()));
                }
            }
            ItemRef::Layer(child) | ItemRef::Leaf(child) => lease_shapes(child, leases),
            ItemRef::Run(_) | ItemRef::Draw(_) | ItemRef::Text(_) => {}
        }
    }
}

// Runs the passes, while the packets of the last `in_flight` passes hold the
// shapes they drew, and returns the store's heap after each pass. Checks
// after each pass that the scene paints what a fresh one does, and that
// what a packet holds stays as it was drawn.
fn churn(in_flight: usize) -> Vec<usize> {
    let pass = Rc::default();
    let mut composition = churning_boxes(Rc::clone(&pass));
    let root = composition.root().expect("a root");
    let viewport = Size::new(240.0, 240.0);
    let mut scene = initial_graph(&mut composition, root, viewport);
    let state = pass.borrow().expect("the state");
    let mut packets: VecDeque<Vec<(Arc<ShapeRecorder>, u64)>> = VecDeque::new();
    (1..=PASSES)
        .map(|number| {
            if in_flight > 0 {
                let mut leases = Vec::new();
                lease_shapes(scene.root_layer().expect("a scene"), &mut leases);
                packets.push_back(
                    leases
                        .into_iter()
                        .map(|lease| {
                            let print = lease.fingerprint();
                            (lease, print)
                        })
                        .collect(),
                );
                if packets.len() > in_flight {
                    packets.pop_front();
                }
            }
            state.set(number);
            update_scene(&mut composition, root, viewport, &mut scene);
            assert_paints_as_fresh(&scene, &mut composition, number);
            for (lease, print) in packets.iter().flatten() {
                assert_eq!(
                    lease.fingerprint(),
                    *print,
                    "pass {number}: a packet in flight draws what it held"
                );
            }
            scene.store().heap_bytes()
        })
        .collect()
}

fn assert_paints_as_fresh(scene: &Scene, composition: &mut TestComposition, pass: u32) {
    let root = composition.root().expect("a root");
    assert_eq!(
        solid_rects(scene),
        solid_rects(&fresh_graph(composition, root)),
        "pass {pass}: the updated scene paints the rects a fresh one does"
    );
}

fn assert_flat_after_settling(heap: &[usize]) {
    let settled = heap[SETTLED - 1];
    let peak = heap[SETTLED..].iter().copied().max().unwrap_or(settled);
    assert!(
        peak <= settled + SLACK,
        "the store holds {settled} bytes after {SETTLED} passes and up to {peak} after; \
         heap after each pass: {heap:?}"
    );
}

#[test]
fn backgrounds_drawn_anew_beside_staying_ones_keep_the_store_heap_flat() {
    assert_flat_after_settling(&churn(0));
}

#[test]
fn backgrounds_drawn_anew_while_packets_hold_their_shapes_keep_the_store_heap_flat() {
    assert_flat_after_settling(&churn(2));
}
