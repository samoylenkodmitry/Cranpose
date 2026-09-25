use std::{cell::Cell, rc::Rc};

use cranpose_core::NodeId;
use cranpose_ui_graphics::Size as ViewportSize;

use super::*;

fn modal_box(
    size: f32,
    hidden: bool,
    id: &Rc<Cell<Option<NodeId>>>,
    content: impl FnMut() + 'static,
) {
    id.set(Some(Box(
        Modifier::empty()
            .size_points(size, size)
            .semantics(move |config| {
                config.is_modal = true;
                config.hidden = hidden;
            }),
        BoxSpec::default(),
        content,
    )));
}

/// The top modal the walk finds and the root the semantics tree takes, for
/// the composition `build` makes.
fn walk_and_tree(build: impl FnMut() + 'static) -> (Option<NodeId>, NodeId, bool) {
    let mut composition = run_test_composition(build);
    let root = composition.root().expect("root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    measure_layout(
        &mut applier,
        root,
        ViewportSize {
            width: 320.0,
            height: 480.0,
        },
    )
    .expect("layout");
    cranpose_core::Applier::get_mut(&mut *applier, root)
        .expect("root node")
        .mark_needs_semantics();
    let walked = crate::top_modal_from_applier(&mut applier, root).expect("walk");
    let still_dirty = crate::tree_needs_semantics(&mut *applier, root).expect("dirty flag");
    let tree_root = crate::build_semantics_tree_from_applier(&mut applier, root)
        .expect("semantics tree")
        .expect("semantics present")
        .root()
        .node_id;
    applier.clear_runtime_handle();
    (walked, tree_root, still_dirty)
}

#[test]
fn the_walk_finds_the_modal_the_semantics_tree_is_rooted_at() {
    let first = Rc::new(Cell::new(None));
    let outer = Rc::new(Cell::new(None));
    let inner = Rc::new(Cell::new(None));
    let hidden = Rc::new(Cell::new(None));
    let empty = Rc::new(Cell::new(None));
    let ids = (
        Rc::clone(&first),
        Rc::clone(&outer),
        Rc::clone(&inner),
        Rc::clone(&hidden),
        Rc::clone(&empty),
    );
    let (walked, tree_root, still_dirty) = walk_and_tree(move || {
        let (first, outer, inner, hidden, empty) = ids.clone();
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            modal_box(40.0, false, &first, || {});
            let inner = Rc::clone(&inner);
            modal_box(80.0, false, &outer, move || {
                modal_box(20.0, false, &inner, || {
                    Text("Inside", Modifier::empty(), TextStyle::default());
                });
            });
            modal_box(40.0, true, &hidden, || {});
            modal_box(0.0, false, &empty, || {});
        });
    });
    assert_eq!(
        walked,
        inner.get(),
        "the last visible modal, innermost first"
    );
    assert_eq!(walked, Some(tree_root));
    assert!(
        still_dirty,
        "the walk leaves the semantics tree to be built"
    );
}

#[test]
fn without_a_modal_the_walk_finds_nothing() {
    let (walked, tree_root, _) = walk_and_tree(|| {
        Column(Modifier::empty(), ColumnSpec::default(), || {
            Text("Plain", Modifier::empty(), TextStyle::default());
            Box(
                Modifier::empty().size_points(10.0, 10.0),
                BoxSpec::default(),
                || {},
            );
        });
    });
    assert_eq!(walked, None);
    assert_ne!(tree_root, 0);
}

fn chain_reach(modifier: &Modifier) -> cranpose_foundation::SemanticsReach {
    let mut handle = crate::modifier::ModifierChainHandle::new();
    let _ = handle.update(modifier);
    crate::modifier::semantics_reach_of_chain(handle.chain())
}

#[test]
fn a_chains_reach_joins_every_semantics_modifier_in_it() {
    let modifier = Modifier::empty()
        .semantics(|config| config.is_modal = true)
        .padding(2.0)
        .semantics(|config| config.hidden = true);
    assert_eq!(
        chain_reach(&modifier),
        cranpose_foundation::SemanticsReach {
            is_modal: true,
            hidden: true,
        }
    );
    assert_eq!(
        chain_reach(&Modifier::empty().padding(2.0)),
        cranpose_foundation::SemanticsReach::default()
    );
}
