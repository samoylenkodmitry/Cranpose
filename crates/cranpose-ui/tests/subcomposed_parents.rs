// A parent node of an app's own type that a subcomposition composes for the
// first time takes its children with the list it held, which is none: the
// type implements only the single-child operations of `Node`, so the
// default list update inserts each child. The parent lists every child in
// order and each child names the parent, also after the slot composes
// more children.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{
    Composition, MemoryApplier, MutableState, Node, NodeId, SlotId, location_key, pop_parent,
    push_parent, with_current_composer,
};
use cranpose_ui::{
    AppContext, LayoutEngine, MeasureResult, Modifier, Size, SubcomposeMeasureScope,
};
use smallvec::SmallVec;

#[derive(Default)]
struct List {
    children: Vec<NodeId>,
}

impl Node for List {
    fn insert_child(&mut self, child: NodeId) -> bool {
        if self.children.contains(&child) {
            return false;
        }
        self.children.push(child);
        true
    }

    fn remove_child(&mut self, child: NodeId) -> bool {
        let Some(at) = self.children.iter().position(|&id| id == child) else {
            return false;
        };
        self.children.remove(at);
        true
    }

    fn move_child(&mut self, from: usize, to: usize) {
        if from < self.children.len() {
            let child = self.children.remove(from);
            self.children.insert(to.min(self.children.len()), child);
        }
    }

    fn collect_children_into(&self, out: &mut SmallVec<[NodeId; 8]>) {
        out.clear();
        out.extend_from_slice(&self.children);
    }
}

#[derive(Default)]
struct Item {
    parent: Option<NodeId>,
}

impl Node for Item {
    fn on_attached_to_parent(&mut self, parent: NodeId) {
        self.parent = Some(parent);
    }

    fn on_removed_from_parent(&mut self) {
        self.parent = None;
    }

    fn parent(&self) -> Option<NodeId> {
        self.parent
    }
}

// The list node and its items, as the slot emitted them.
#[derive(Default)]
struct Emitted {
    list: Cell<Option<NodeId>>,
    items: RefCell<Vec<NodeId>>,
}

fn compose(context: &Rc<AppContext>, composition: &mut Composition<MemoryApplier>) {
    context.enter(|| {
        let root = composition.root().expect("the subcompose layout");
        let runtime = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(runtime);
        applier
            .compute_layout(root, Size::new(100.0, 100.0))
            .expect("the layout");
        applier.clear_runtime_handle();
    });
}

fn render(
    context: &Rc<AppContext>,
    composition: &mut Composition<MemoryApplier>,
    count: MutableState<usize>,
    emitted: &Rc<Emitted>,
) {
    let emitted = Rc::clone(emitted);
    context
        .enter(|| {
            composition.render(location_key(file!(), line!(), column!()), move || {
                let emitted = Rc::clone(&emitted);
                cranpose_ui::SubcomposeLayout(Modifier::empty(), move |scope, constraints| {
                    let emitted = Rc::clone(&emitted);
                    scope.subcompose(SlotId::new(0), (), move || {
                        let list =
                            with_current_composer(|composer| composer.emit_node(List::default));
                        emitted.list.set(Some(list));
                        push_parent(list);
                        let items = (0..count.get())
                            .map(|_| {
                                with_current_composer(|composer| composer.emit_node(Item::default))
                            })
                            .collect();
                        *emitted.items.borrow_mut() = items;
                        pop_parent();
                    });
                    let (width, height) = constraints.constrain(0.0, 0.0);
                    MeasureResult::new(Size::new(width, height), Vec::new())
                });
            })
        })
        .expect("the composition");
    compose(context, composition);
}

fn assert_listed(composition: &mut Composition<MemoryApplier>, emitted: &Emitted) {
    let list = emitted.list.get().expect("the slot emitted a list");
    let items = emitted.items.borrow().clone();
    let mut applier = composition.applier_mut();
    let children = applier
        .with_node::<List, _>(list, |list| list.children.clone())
        .expect("the list node");
    assert_eq!(children, items, "the list holds its items in order");
    for item in items {
        let parent = applier
            .with_node::<Item, _>(item, |item| item.parent)
            .expect("an item node");
        assert_eq!(parent, Some(list), "item {item} names the list");
    }
}

#[test]
fn a_parent_of_an_app_type_takes_its_first_children_in_a_subcomposition() {
    let context = AppContext::new();
    let mut composition = Composition::new(MemoryApplier::new());
    let count = MutableState::with_runtime(3, composition.runtime_handle());
    let emitted = Rc::new(Emitted::default());

    render(&context, &mut composition, count, &emitted);
    assert_eq!(emitted.items.borrow().len(), 3);
    assert_listed(&mut composition, &emitted);

    count.set(5);
    render(&context, &mut composition, count, &emitted);
    assert_eq!(emitted.items.borrow().len(), 5);
    assert_listed(&mut composition, &emitted);
}
