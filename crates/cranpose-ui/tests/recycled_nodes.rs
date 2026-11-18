use std::{
    cell::Cell,
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_core::{MutableState, NodeId, collections::map::HashMap};
use cranpose_foundation::{
    DelegatableNode, ModifierNode, ModifierNodeContext, ModifierNodeElement, NodeState,
};
use cranpose_ui::{
    Box, BoxSpec, LayoutNode, Modifier, ModifierNodeSlices, Rect, Row, RowSpec, Size,
    TestComposition,
};

use crate::{directional_layout::tagged, text_measured_layout_integration::layout_composition};

// The kinds of node the rounds show in turn, at one place of a row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    // A weighted box that clips inside its padding, takes clicks and holds
    // a child.
    Weighted,
    // A box of its own size that clips inside a wider padding.
    Padded,
    // A box of its own size alone.
    Plain,
}

impl Kind {
    fn of(round: u32) -> Self {
        [Self::Weighted, Self::Padded, Self::Plain][round as usize % 3]
    }

    fn show(self, made_for: MadeFor) -> NodeId {
        let made_for = Modifier::with_element(made_for);
        match self {
            Self::Weighted => Box(
                made_for
                    .weight(1.0)
                    .height(40.0)
                    .padding(5.0)
                    .clip_to_bounds()
                    .clickable(|_| {})
                    .content_description("node"),
                BoxSpec::default(),
                || {
                    Box(
                        Modifier::empty().size(Size::new(7.0, 7.0)),
                        BoxSpec::default(),
                        || {},
                    );
                },
            ),
            Self::Padded => Box(
                made_for
                    .size(PADDED_SIZE)
                    .padding(12.0)
                    .clip_to_bounds()
                    .content_description("node"),
                BoxSpec::default(),
                || {},
            ),
            Self::Plain => Box(
                made_for
                    .size(Size::new(40.0, 25.0))
                    .content_description("node"),
                BoxSpec::default(),
                || {},
            ),
        }
    }

    fn size(self) -> Size {
        match self {
            Self::Weighted => Size::new(200.0, 40.0),
            Self::Padded => PADDED_SIZE,
            Self::Plain => Size::new(40.0, 25.0),
        }
    }
}

const PADDED_SIZE: Size = Size::new(60.0, 40.0);

// A modifier whose node tells, as it attaches, the round that made it: a
// node kept from the chain of an earlier round tells that round.
#[derive(Debug)]
struct MadeFor {
    round: u32,
    told: Rc<Cell<Option<u32>>>,
}

impl PartialEq for MadeFor {
    fn eq(&self, other: &Self) -> bool {
        self.round == other.round
    }
}

impl Hash for MadeFor {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u32(self.round);
    }
}

#[derive(Debug)]
struct MadeForNode {
    round: u32,
    told: Rc<Cell<Option<u32>>>,
    state: NodeState,
}

impl DelegatableNode for MadeForNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for MadeForNode {
    fn on_attach(&mut self, _context: &mut dyn ModifierNodeContext) {
        self.told.set(Some(self.round));
    }
}

impl ModifierNodeElement for MadeFor {
    type Node = MadeForNode;

    fn create(&self) -> MadeForNode {
        MadeForNode {
            round: self.round,
            told: Rc::clone(&self.told),
            state: NodeState::new(),
        }
    }

    fn update(&self, _node: &mut MadeForNode) {}
}

// A row that shows the node of the current round: the round's state, the
// id of the node it shows and the round its modifier node tells.
struct Rounds {
    composition: TestComposition,
    round: MutableState<u32>,
    shown: Rc<Cell<Option<NodeId>>>,
    told: Rc<Cell<Option<u32>>>,
}

impl Rounds {
    fn compose() -> Self {
        let round_handle: Rc<Cell<Option<MutableState<u32>>>> = Rc::default();
        let shown: Rc<Cell<Option<NodeId>>> = Rc::default();
        let told: Rc<Cell<Option<u32>>> = Rc::default();
        let composition = {
            let round_handle = Rc::clone(&round_handle);
            let shown = Rc::clone(&shown);
            let told = Rc::clone(&told);
            cranpose_ui::run_test_composition(move || {
                let round =
                    cranpose_core::remember(|| cranpose_core::mutableStateOf(0_u32)).with(|s| *s);
                round_handle.set(Some(round));
                let value = round.value();
                let shown = Rc::clone(&shown);
                let told = Rc::clone(&told);
                Row(
                    Modifier::empty().width(200.0),
                    RowSpec::default(),
                    move || {
                        let made_for = MadeFor {
                            round: value,
                            told: Rc::clone(&told),
                        };
                        shown.set(Some(cranpose_core::key(value, || {
                            Kind::of(value).show(made_for)
                        })));
                    },
                );
            })
        };
        Self {
            composition,
            round: round_handle.get().expect("the round state"),
            shown,
            told,
        }
    }
}

// Each round shows its kind of node under a key of its own, so the node of
// a round is new and takes the place, the box and the parts of a node an
// earlier round let go of. The new node shows nothing of that node: not its
// size, its weight, its child, its clicks, its clip or a modifier node of its
// chain. A handle of the slices of a node that left, held on, reads that
// node's rects, never the node that took its place.
#[test]
fn a_node_that_takes_a_recycled_place_shows_nothing_of_the_node_before() {
    let Rounds {
        mut composition,
        round,
        shown,
        told,
    } = Rounds::compose();
    let mut kinds: HashMap<NodeId, Kind> = HashMap::default();
    let mut taken_from_another_kind = 0;
    let mut held: Option<Rc<ModifierNodeSlices>> = None;
    for value in 0..24 {
        if value > 0 {
            round.set_value(value);
            while composition.process_invalid_scopes().expect("recompose") {}
        }
        let kind = Kind::of(value);
        let layout = layout_composition(&mut composition, Size::new(200.0, 200.0));
        assert_eq!(
            told.get(),
            Some(value),
            "round {value}: the round that made the attached modifier node"
        );
        let node = tagged(layout.root(), "node");
        assert_eq!(
            node.rect.size(),
            kind.size(),
            "round {value}: the size of {kind:?}"
        );
        assert_eq!(
            node.children.len(),
            usize::from(kind == Kind::Weighted),
            "round {value}: the children of {kind:?}"
        );
        let id = shown.get().expect("the shown node");
        let (children, clicks, slices) = composition
            .applier_mut()
            .with_node::<LayoutNode, _>(id, |node| {
                (
                    node.children().len(),
                    node.has_pointer_input_modifier_nodes(),
                    node.modifier_slices_snapshot(),
                )
            })
            .expect("the shown node is a layout node");
        assert_eq!(
            children,
            usize::from(kind == Kind::Weighted),
            "round {value}"
        );
        assert_eq!(clicks, kind == Kind::Weighted, "round {value}: clicks");
        assert_eq!(
            slices.clip_to_bounds(),
            kind != Kind::Plain,
            "round {value}: clip"
        );
        if let Some(before) = kinds.insert(id, kind)
            && before != kind
        {
            taken_from_another_kind += 1;
        }
        if kind == Kind::Padded && held.is_none() && value > 6 {
            held = Some(slices);
        }
        if let Some(held) = &held {
            assert_eq!(
                held.layer_bounds(PADDED_SIZE),
                Rect {
                    x: 12.0,
                    y: 12.0,
                    width: 36.0,
                    height: 16.0
                },
                "round {value}: a held handle reads the rects of its own node"
            );
        }
    }
    assert!(
        taken_from_another_kind > 0,
        "no round took the place of a node of another kind"
    );
}
