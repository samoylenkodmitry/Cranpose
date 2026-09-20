use super::*;

#[derive(Clone, Copy)]
struct Probe {
    count: MutableState<i32>,
    label: MutableState<&'static str>,
}

impl Probe {
    fn new() -> Self {
        Probe {
            count: mutableStateOf(0),
            label: mutableStateOf("first"),
        }
    }
}

fn render_probe(
    composition: &mut Composition<MemoryApplier>,
    root_key: Key,
    builds: &Rc<Cell<usize>>,
    seen: &Rc<Cell<Option<Probe>>>,
    show: MutableState<bool>,
) {
    let builds = Rc::clone(builds);
    let seen = Rc::clone(seen);
    composition
        .render(root_key, move || {
            if show.value() {
                let probe = remember(|| {
                    builds.set(builds.get() + 1);
                    Probe::new()
                })
                .with(|probe| *probe);
                seen.set(Some(probe));
            }
        })
        .expect("render the state holder");
    assert_composition_valid(composition);
}

#[test]
fn a_remembered_holder_is_built_once_and_comes_back_the_same_object() {
    let mut composition = test_composition();
    let show = MutableState::with_runtime(true, composition.runtime_handle());
    let builds = Rc::new(Cell::new(0));
    let seen = Rc::new(Cell::new(None::<Probe>));
    let root_key = location_key(file!(), line!(), column!());

    render_probe(&mut composition, root_key, &builds, &seen, show);
    let first = seen.get().expect("the holder is built on the first pass");
    assert_eq!(builds.get(), 1, "the first pass builds the holder");
    first.count.set_value(7);
    first.label.set_value("second");

    render_probe(&mut composition, root_key, &builds, &seen, show);
    let again = seen.get().expect("the holder is there on the second pass");
    assert_eq!(builds.get(), 1, "a later pass must not build it again");
    assert_eq!(
        again.count, first.count,
        "the holder that comes back is the one that was built"
    );
    assert_eq!(again.label, first.label);
    assert_eq!(again.count.value(), 7, "its states keep their values");
    assert_eq!(again.label.value(), "second");
}

#[test]
fn a_remembered_holder_releases_its_states_when_it_leaves() {
    let mut composition = test_composition();
    let show = MutableState::with_runtime(true, composition.runtime_handle());
    let builds = Rc::new(Cell::new(0));
    let seen = Rc::new(Cell::new(None::<Probe>));
    let root_key = location_key(file!(), line!(), column!());

    render_probe(&mut composition, root_key, &builds, &seen, show);
    let probe = seen.get().expect("the holder is built on the first pass");
    assert!(probe.count.is_alive());
    assert!(probe.label.is_alive());

    show.set_value(false);
    render_probe(&mut composition, root_key, &builds, &seen, show);
    assert!(
        !probe.count.is_alive(),
        "the slot owns the states and lets them go with it"
    );
    assert!(!probe.label.is_alive());
}

#[test]
fn the_runtime_never_takes_ownership_of_a_remembered_holder_s_states() {
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let show = MutableState::with_runtime(true, runtime.clone());
    let builds = Rc::new(Cell::new(0));
    let seen = Rc::new(Cell::new(None::<Probe>));
    let root_key = location_key(file!(), line!(), column!());
    let owned_by_runtime = runtime.debug_stats().external_state_owners_len;

    for _ in 0..8 {
        render_probe(&mut composition, root_key, &builds, &seen, show);
    }

    assert_eq!(
        runtime.debug_stats().external_state_owners_len,
        owned_by_runtime,
        "a state made while a remember builds its value belongs to the slot, \
         not to the runtime; parking it on the runtime is what used to make \
         the Kotlin shape leak here"
    );
}
