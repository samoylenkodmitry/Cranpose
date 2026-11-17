use std::{cell::Cell, rc::Rc};

use cranpose_core::{MutableState, NodeId, collections::map::HashMap};
use cranpose_ui::{
    Box, BoxSpec, LayoutNode, Modifier, ModifierNodeSlices, Rect, Row, RowSpec, Size,
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

    fn show(self) -> NodeId {
        match self {
            Self::Weighted => Box(
                Modifier::empty()
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
                Modifier::empty()
                    .size(PADDED_SIZE)
                    .padding(12.0)
                    .clip_to_bounds()
                    .content_description("node"),
                BoxSpec::default(),
                || {},
            ),
            Self::Plain => Box(
                Modifier::empty()
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

// Each round shows its kind of node under a key of its own, so the node of
// a round is new and takes the place, the box and the parts of a node an
// earlier round let go of. The new node shows nothing of that node: not its
// size, its weight, its child, its clicks or its clip. A handle of the
// slices of a node that left, held on, reads that node's rects, never the
// node that took its place.
#[test]
fn a_node_that_takes_a_recycled_place_shows_nothing_of_the_node_before() {
    let round_handle: Rc<Cell<Option<MutableState<u32>>>> = Rc::default();
    let shown: Rc<Cell<Option<NodeId>>> = Rc::default();
    let mut composition = {
        let round_handle = Rc::clone(&round_handle);
        let shown = Rc::clone(&shown);
        cranpose_ui::run_test_composition(move || {
            let round =
                cranpose_core::remember(|| cranpose_core::mutableStateOf(0_u32)).with(|s| *s);
            round_handle.set(Some(round));
            let value = round.value();
            let shown = Rc::clone(&shown);
            Row(
                Modifier::empty().width(200.0),
                RowSpec::default(),
                move || {
                    shown.set(Some(cranpose_core::key(value, || Kind::of(value).show())));
                },
            );
        })
    };
    let round = round_handle.get().expect("the round state");
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
