// Keyed cards that enter a column before other cards, every record of a new
// card landing ahead of the records of the cards after it: the later cards
// keep their nodes, their order and their remembered state, whatever the new
// card's body does while it composes.

use std::{
    cell::{Cell, RefCell},
    fmt::Write,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

use cranpose_core::{
    Composition, MemoryApplier, MutableState, NodeId, key, location_key, movable, pop_parent,
    push_parent, remember, with_current_composer,
};

use super::{
    ordered_parent::{FixtureLeaf, ParentTracked},
    tracked_parent::TrackedParent,
};

const ROWS: u32 = 3;
const TILES: u32 = 6;
// The row before which a card's twist acts: the rows before it insert more
// records than a new subtree inserts before the records after it wait aside.
const TWIST_ROW: u32 = 2;
// The rows of a tall card: more groups than the anchors of the groups after
// it may trail by before they are written again.
const TALL_ROWS: u32 = 40;

struct Label(RefCell<String>);

impl FixtureLeaf for Label {}

// What a card's body does besides composing its header and rows.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Twist {
    Plain,
    // Catches the panic of a part it shows before `TWIST_ROW`.
    Panic,
    // Returns before `TWIST_ROW`.
    ReturnEarly,
    // Shows the moving content after its rows.
    ShowMoving,
    // Shows `TALL_ROWS` rows.
    Tall,
}

#[derive(Clone)]
struct Screen {
    // The keys of the sections, each with the keys of its cards.
    sections: MutableState<Vec<(u32, Vec<u32>)>>,
    // Read by every header.
    epoch: MutableState<u32>,
    twists: MutableState<Vec<(u32, Twist)>>,
    births: Rc<Cell<u32>>,
}

impl PartialEq for Screen {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.births, &other.births)
    }
}

impl Screen {
    fn twist(&self, card: u32) -> Twist {
        self.twists
            .get()
            .iter()
            .find(|(id, _)| *id == card)
            .map_or(Twist::Plain, |(_, twist)| *twist)
    }

    fn birth(&self) -> u32 {
        let born = self.births.get();
        self.births.set(born + 1);
        born
    }
}

fn parent_node() -> NodeId {
    with_current_composer(|composer| composer.emit_node(TrackedParent::default))
}

// A label node showing `text`, set again each time the body runs.
fn leaf(text: String) {
    with_current_composer(|composer| {
        let id = composer.emit_node(|| ParentTracked::new(Label(RefCell::default())));
        composer
            .with_node_mut::<ParentTracked<Label>, _>(id, |leaf| *leaf.0.borrow_mut() = text)
            .expect("a label node");
    });
}

#[cranpose_macros::composable]
fn Column(screen: Screen) {
    push_parent(parent_node());
    for (section, cards) in screen.sections.get() {
        let screen = screen.clone();
        key(section, move || Section(cards, screen));
    }
    pop_parent();
}

#[cranpose_macros::composable]
fn Section(cards: Vec<u32>, screen: Screen) {
    push_parent(parent_node());
    for id in cards {
        let screen = screen.clone();
        key(id, move || Card(id, screen));
    }
    pop_parent();
}

#[cranpose_macros::composable]
fn Card(id: u32, screen: Screen) {
    let born = remember(|| screen.birth()).with(|born| *born);
    let twist = screen.twist(id);
    push_parent(parent_node());
    Header(id, born, screen.epoch);
    let rows = if twist == Twist::Tall {
        TALL_ROWS
    } else {
        ROWS
    };
    for row in 0..rows {
        if row == TWIST_ROW {
            match twist {
                Twist::Panic => {
                    let failed = catch_unwind(AssertUnwindSafe(|| Faulty(id)));
                    assert!(failed.is_err(), "the faulty part panics");
                }
                Twist::ReturnEarly => {
                    pop_parent();
                    return;
                }
                Twist::Plain | Twist::ShowMoving | Twist::Tall => {}
            }
        }
        Row(id, row);
    }
    if twist == Twist::ShowMoving {
        movable("moving", || {
            let born = remember(|| screen.birth()).with(|born| *born);
            leaf(format!("moving#{born}"));
        });
    }
    pop_parent();
}

// A part that shows a label and then panics.
#[cranpose_macros::composable]
fn Faulty(card: u32) {
    leaf(format!("{card}.faulty"));
    panic!("card {card} has a faulty part");
}

#[cranpose_macros::composable]
fn Header(id: u32, born: u32, epoch: MutableState<u32>) {
    leaf(format!("{id}#{born}@{}", epoch.get()));
}

#[cranpose_macros::composable]
fn Row(card: u32, row: u32) {
    push_parent(parent_node());
    for tile in 0..TILES {
        leaf(format!("{card}.{row}.{tile}"));
    }
    pop_parent();
}

// One card as the test expects it: its key, the stamp its first composition
// took, how many rows it shows and the stamp of the moving content it shows.
#[derive(Clone, Copy)]
struct Expected {
    id: u32,
    born: u32,
    rows: u32,
    moving: Option<u32>,
}

fn card(id: u32, born: u32) -> Expected {
    Expected {
        id,
        born,
        rows: ROWS,
        moving: None,
    }
}

fn expected_shape(sections: &[&[Expected]], epoch: u32) -> String {
    let mut out = String::from("[");
    for cards in sections {
        out.push('[');
        for card in *cards {
            write!(out, "[{}#{}@{epoch} ", card.id, card.born).expect("write to a string");
            for row in 0..card.rows {
                out.push('[');
                for tile in 0..TILES {
                    write!(out, "{}.{row}.{tile} ", card.id).expect("write to a string");
                }
                out.push(']');
            }
            if let Some(born) = card.moving {
                write!(out, "moving#{born} ").expect("write to a string");
            }
            out.push(']');
        }
        out.push(']');
    }
    out.push(']');
    out
}

// Writes the tree under `node`, each parent's children in brackets, into
// `out`, and the label nodes in tree order into `labels`.
fn shape_of(
    applier: &mut MemoryApplier,
    node: NodeId,
    out: &mut String,
    labels: &mut Vec<(String, NodeId)>,
) {
    match applier.with_node::<TrackedParent, _>(node, |parent| parent.inner.children.clone()) {
        Ok(children) => {
            out.push('[');
            for child in children {
                shape_of(applier, child, out, labels);
            }
            out.push(']');
        }
        Err(_) => {
            let text = applier
                .with_node::<ParentTracked<Label>, _>(node, |leaf| leaf.0.borrow().clone())
                .expect("every node is a parent or a label");
            write!(out, "{text} ").expect("write to a string");
            labels.push((text, node));
        }
    }
}

struct Harness {
    composition: Composition<MemoryApplier>,
    screen: Screen,
}

impl Harness {
    fn new(sections: Vec<(u32, Vec<u32>)>, twists: Vec<(u32, Twist)>) -> Self {
        let composition = Composition::new(MemoryApplier::new());
        let runtime = composition.runtime_handle();
        let screen = Screen {
            sections: MutableState::with_runtime(sections, runtime.clone()),
            epoch: MutableState::with_runtime(0, runtime.clone()),
            twists: MutableState::with_runtime(twists, runtime),
            births: Rc::default(),
        };
        let mut harness = Self {
            composition,
            screen,
        };
        harness.render();
        harness
    }

    // Composes the whole column from its root.
    fn render(&mut self) {
        let content = self.screen.clone();
        self.composition
            .render(location_key(file!(), line!(), column!()), move || {
                Column(content.clone());
            })
            .expect("composition from the root");
    }

    fn column(&self) -> Vec<(u32, Vec<u32>)> {
        self.screen.sections.get()
    }

    fn show(&mut self, sections: Vec<(u32, Vec<u32>)>) {
        self.screen.sections.set(sections);
        self.recompose();
    }

    fn recompose(&mut self) {
        while self
            .composition
            .process_invalid_scopes()
            .expect("recomposition")
        {}
    }

    // Recomposes every header, each through its own scope.
    fn next_epoch(&mut self) -> u32 {
        let epoch = self.screen.epoch.get() + 1;
        self.screen.epoch.set(epoch);
        self.recompose();
        epoch
    }

    fn shape(&mut self) -> String {
        self.shape_and_labels().0
    }

    fn shape_and_labels(&mut self) -> (String, Vec<(String, NodeId)>) {
        let root = self.composition.root().expect("the column node");
        let mut out = String::new();
        let mut labels = Vec::new();
        shape_of(
            &mut self.composition.applier_mut(),
            root,
            &mut out,
            &mut labels,
        );
        (out, labels)
    }

    fn label_node(&mut self, text: &str) -> NodeId {
        self.shape_and_labels()
            .1
            .into_iter()
            .find_map(|(label, node)| (label == text).then_some(node))
            .unwrap_or_else(|| panic!("no node shows {text}"))
    }

    // The card nodes of every section, in order.
    fn card_nodes(&mut self) -> Vec<NodeId> {
        let root = self.composition.root().expect("the column node");
        let mut applier = self.composition.applier_mut();
        let sections = applier
            .with_node::<TrackedParent, _>(root, |column| column.inner.children.clone())
            .expect("the column is a parent");
        sections
            .into_iter()
            .flat_map(|section| {
                applier
                    .with_node::<TrackedParent, _>(section, |section| {
                        section.inner.children.clone()
                    })
                    .expect("a section is a parent")
            })
            .collect()
    }
}

fn one_section(cards: &[u32]) -> Vec<(u32, Vec<u32>)> {
    vec![(0, cards.to_vec())]
}

#[test]
fn a_card_entering_the_front_keeps_the_cards_after_it() {
    let mut harness = Harness::new(one_section(&[1, 2, 3]), Vec::new());
    let kept = harness.card_nodes();

    harness.show(one_section(&[0, 1, 2, 3]));

    let cards = [card(0, 3), card(1, 0), card(2, 1), card(3, 2)];
    assert_eq!(harness.shape(), expected_shape(&[&cards], 0));
    assert_eq!(harness.card_nodes()[1..], kept[..]);
    let epoch = harness.next_epoch();
    assert_eq!(harness.shape(), expected_shape(&[&cards], epoch));
}

#[test]
fn several_cards_enter_between_kept_cards_in_one_pass() {
    let mut harness = Harness::new(one_section(&[1, 2, 3]), Vec::new());
    let kept = harness.card_nodes();

    harness.show(one_section(&[10, 1, 11, 12, 2, 13, 3, 14]));

    let cards = [
        card(10, 3),
        card(1, 0),
        card(11, 4),
        card(12, 5),
        card(2, 1),
        card(13, 6),
        card(3, 2),
        card(14, 7),
    ];
    assert_eq!(harness.shape(), expected_shape(&[&cards], 0));
    let nodes = harness.card_nodes();
    assert_eq!([nodes[1], nodes[4], nodes[6]], kept[..]);
    let epoch = harness.next_epoch();
    assert_eq!(harness.shape(), expected_shape(&[&cards], epoch));
}

#[test]
fn cards_move_and_leave_in_the_pass_a_card_enters() {
    cards_move_and_leave_around_an_entering_card(Twist::Plain, ROWS);
}

#[test]
fn cards_move_and_leave_in_the_pass_a_tall_card_enters() {
    cards_move_and_leave_around_an_entering_card(Twist::Tall, TALL_ROWS);
}

fn cards_move_and_leave_around_an_entering_card(twist: Twist, rows: u32) {
    let before: Vec<u32> = (1..=20).collect();
    let mut harness = Harness::new(one_section(&before), vec![(0, twist)]);
    let kept = harness.card_nodes();
    // Card 18 moves to the front past the 16 siblings a linear search
    // reads, card 0 enters after it, card 17 leaves and 20 swaps with 19.
    let mut after = vec![18, 0];
    after.extend(1..=16);
    after.extend([20, 19]);

    harness.show(one_section(&after));

    let cards: Vec<Expected> = after
        .iter()
        .map(|&id| match id {
            0 => Expected {
                rows,
                ..card(0, 20)
            },
            id => card(id, id - 1),
        })
        .collect();
    assert_eq!(
        harness.shape(),
        expected_shape(&[&cards], 0),
        "the column after the pass"
    );
    let node_of = |id: u32| kept[id as usize - 1];
    let expected_nodes: Vec<NodeId> = after
        .iter()
        .filter(|&&id| id != 0)
        .map(|&id| node_of(id))
        .collect();
    let mut nodes = harness.card_nodes();
    nodes.remove(1);
    assert_eq!(nodes, expected_nodes, "the kept cards keep their nodes");
    let epoch = harness.next_epoch();
    assert_eq!(
        harness.shape(),
        expected_shape(&[&cards], epoch),
        "every header recomposed"
    );
}

#[test]
fn a_new_section_of_new_cards_enters_before_kept_sections() {
    let mut harness = Harness::new(vec![(1, vec![1, 2]), (2, vec![3])], Vec::new());
    let kept = harness.card_nodes();

    let mut sections = vec![(0, vec![10, 11])];
    sections.extend(harness.column());
    harness.show(sections);

    let new = [card(10, 3), card(11, 4)];
    let first = [card(1, 0), card(2, 1)];
    let second = [card(3, 2)];
    assert_eq!(harness.shape(), expected_shape(&[&new, &first, &second], 0));
    assert_eq!(harness.card_nodes()[2..], kept[..]);

    // A card enters the front of a kept section, deeper in the table.
    harness.show(vec![(0, vec![10, 11]), (1, vec![12, 1, 2]), (2, vec![3])]);

    let first = [card(12, 5), card(1, 0), card(2, 1)];
    assert_eq!(harness.shape(), expected_shape(&[&new, &first, &second], 0));
    let epoch = harness.next_epoch();
    assert_eq!(
        harness.shape(),
        expected_shape(&[&new, &first, &second], epoch)
    );
}

#[test]
fn moving_content_follows_from_a_later_card_into_a_card_entering_the_front() {
    let mut harness = Harness::new(one_section(&[1, 2, 3]), vec![(3, Twist::ShowMoving)]);
    let mut shown = [card(1, 0), card(2, 1), card(3, 2)];
    shown[2].moving = Some(3);
    assert_eq!(harness.shape(), expected_shape(&[&shown], 0));
    let kept = harness.card_nodes();
    let moving = harness.label_node("moving#3");

    harness.screen.twists.set(vec![(0, Twist::ShowMoving)]);
    harness.show(one_section(&[0, 1, 2, 3]));

    let mut cards = [card(0, 4), card(1, 0), card(2, 1), card(3, 2)];
    cards[0].moving = Some(3);
    assert_eq!(harness.shape(), expected_shape(&[&cards], 0));
    assert_eq!(harness.label_node("moving#3"), moving);
    assert_eq!(harness.card_nodes()[1..], kept[..]);
    let epoch = harness.next_epoch();
    assert_eq!(harness.shape(), expected_shape(&[&cards], epoch));
}

#[test]
fn a_panic_unwinding_inside_an_entering_card_keeps_the_cards_after_it() {
    let mut harness = Harness::new(one_section(&[1, 2, 3]), vec![(0, Twist::Panic)]);
    let kept = harness.card_nodes();

    harness.show(one_section(&[0, 1, 2, 3]));

    let cards = [card(0, 3), card(1, 0), card(2, 1), card(3, 2)];
    let last_tile_before = format!("0.{}.{} ]", TWIST_ROW - 1, TILES - 1);
    let with_faulty = expected_shape(&[&cards], 0).replacen(
        &last_tile_before,
        &format!("{last_tile_before}0.faulty "),
        1,
    );
    assert_eq!(harness.shape(), with_faulty);
    assert_eq!(harness.card_nodes()[1..], kept[..]);

    harness.screen.twists.set(Vec::new());
    let epoch = harness.next_epoch();
    assert_eq!(harness.shape(), expected_shape(&[&cards], epoch));
}

#[test]
fn a_card_that_returns_early_while_it_enters_keeps_the_cards_after_it() {
    let mut harness = Harness::new(one_section(&[1, 2, 3]), vec![(0, Twist::ReturnEarly)]);
    let kept = harness.card_nodes();

    harness.show(one_section(&[0, 1, 2, 3]));

    let mut cards = [card(0, 3), card(1, 0), card(2, 1), card(3, 2)];
    cards[0].rows = TWIST_ROW;
    assert_eq!(harness.shape(), expected_shape(&[&cards], 0));
    assert_eq!(harness.card_nodes()[1..], kept[..]);

    harness.screen.twists.set(Vec::new());
    let epoch = harness.next_epoch();
    cards[0].rows = ROWS;
    assert_eq!(harness.shape(), expected_shape(&[&cards], epoch));
}
