use super::*;
use cranpose_core::{location_key, Composition, MemoryApplier, MutableState, NodeId};
use cranpose_foundation::lazy::{remember_lazy_list_state, LazyListScope};

fn render_texts(composition: &mut Composition<MemoryApplier>, root: NodeId) -> Vec<String> {
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let layout = applier
        .compute_layout(
            root,
            Size {
                width: 400.0,
                height: 400.0,
            },
        )
        .expect("layout");
    applier.clear_runtime_handle();
    let renderer = HeadlessRenderer::new();
    let scene = renderer.render(&layout);
    scene
        .operations()
        .iter()
        .filter_map(|op| match op {
            RenderOp::Text { value, .. } => Some(value.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn lazy_list_item_recomposes_on_state_change() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let label_state = MutableState::with_runtime(0u32, runtime.clone());

    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, || {
            let list_state = remember_lazy_list_state();
            LazyColumn(
                Modifier::empty(),
                list_state,
                LazyColumnSpec::default(),
                |scope| {
                    scope.item(Some(0), None, {
                        move || {
                            Text(
                                format!("Animated {}", label_state.value()),
                                Modifier::empty(),
                                TextStyle::default(),
                            );
                        }
                    });
                },
            );
        })
        .expect("initial render");

    let root = composition.root().expect("lazy list root");
    let texts = render_texts(&mut composition, root);
    assert!(
        texts.iter().any(|text| text == "Animated 0"),
        "expected initial text to be rendered in lazy list item"
    );

    label_state.set(1);
    composition
        .process_invalid_scopes()
        .expect("recompose lazy list item");

    let texts = render_texts(&mut composition, root);
    assert!(
        texts.iter().any(|text| text == "Animated 1"),
        "expected updated text to recompose inside lazy list item"
    );
}
