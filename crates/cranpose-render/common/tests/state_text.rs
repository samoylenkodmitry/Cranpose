// A text of a state shows the state's value as it changes, whether the
// widget reads the state itself or takes a source made from it once, and
// whatever the value's formatting does.

use std::{cell::Cell, fmt, rc::Rc};

use cranpose_core::{MutableState, remember, rememberMutableStateOf};
use cranpose_ui::{
    Modifier, Size, TestComposition, Text, TextStyle, run_test_composition,
    widgets::{IntoTextSource, TextSource},
};

use crate::scene_probe::{initial_graph, painted_text};

const VIEWPORT: Size = Size::new(240.0, 120.0);

// The strings the scene of `composition` paints, once it recomposed what
// waits.
fn painted(composition: &mut TestComposition) -> Vec<String> {
    while composition.process_invalid_scopes().expect("recompose") {}
    let root = composition.root().expect("a composed root");
    let scene = initial_graph(composition, root, VIEWPORT);
    let mut out = Vec::new();
    painted_text(scene.root_layer().expect("a scene"), &mut out);
    out
}

// A text the app made into a source before its widget composed, as a
// wrapper type that keeps a source does.
#[derive(Clone, PartialEq)]
struct KeptSource(TextSource);

impl IntoTextSource for KeptSource {
    fn into_text_source(self) -> TextSource {
        self.0
    }
}

#[test]
fn a_source_made_once_from_a_state_follows_the_state() {
    let count: Rc<Cell<Option<MutableState<u32>>>> = Rc::default();
    let kept = Rc::clone(&count);
    let mut composition = run_test_composition(move || {
        let state = rememberMutableStateOf(|| 1u32);
        kept.set(Some(state));
        let source = remember(|| KeptSource(state.into_text_source())).with(Clone::clone);
        Text(source, Modifier::empty(), TextStyle::default());
    });
    assert_eq!(painted(&mut composition), ["1"]);
    count.get().expect("the count").set(2);
    assert_eq!(painted(&mut composition), ["2"]);
}

// A value that writes the state that holds it, once, as it formats.
#[derive(Clone)]
struct SelfWriting {
    value: u32,
    holder: Rc<Cell<Option<MutableState<Self>>>>,
}

impl PartialEq for SelfWriting {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl fmt::Display for SelfWriting {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(state) = self.holder.take() {
            state.set(Self {
                value: self.value + 1,
                holder: Rc::default(),
            });
        }
        write!(formatter, "{}", self.value)
    }
}

#[test]
fn a_value_that_writes_its_state_as_it_formats_shows() {
    let holder: Rc<Cell<Option<MutableState<SelfWriting>>>> = Rc::default();
    let given = Rc::clone(&holder);
    let mut composition = run_test_composition(move || {
        let holder = Rc::clone(&given);
        let state = rememberMutableStateOf(move || SelfWriting {
            value: 1,
            holder: Rc::clone(&holder),
        });
        given.set(Some(state));
        Text(state, Modifier::empty(), TextStyle::default());
    });
    assert_eq!(painted(&mut composition), ["2"]);
}
