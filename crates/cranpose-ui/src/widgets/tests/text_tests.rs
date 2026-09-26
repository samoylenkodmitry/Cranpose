use std::{cell::Cell, rc::Rc};

use cranpose_core::{Composition, MemoryApplier, location_key};

use super::*;
use crate::run_test_composition;

#[test]
fn basic_text_creates_node() {
    let _app_context = crate::render_state::app_context_test_scope();
    let composition = run_test_composition(|| {
        BasicTextWithOptions(
            "Hello",
            Modifier::empty(),
            TextStyle::default(),
            TextLayoutOptions::default(),
        );
    });

    assert!(composition.root().is_some());
}

#[test]
fn text_with_options_creates_node() {
    let _app_context = crate::render_state::app_context_test_scope();
    let composition = run_test_composition(|| {
        TextWithOptions(
            "Hello",
            Modifier::empty(),
            TextStyle::default(),
            TextOptions {
                overflow: TextOverflow::Ellipsis,
                soft_wrap: false,
                max_lines: Some(1),
                ..TextOptions::default()
            },
        );
    });

    assert!(composition.root().is_some());
}

#[test]
fn basic_text_recomposes_when_dynamic_source_changes() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let state = MutableState::with_runtime("Hello".to_string(), runtime);
    let resolutions = Rc::new(Cell::new(0));

    composition
        .render(location_key(file!(), line!(), column!()), {
            let text_state = state;
            let resolutions = Rc::clone(&resolutions);
            move || {
                let text_state = text_state;
                let resolutions = Rc::clone(&resolutions);
                BasicText(
                    DynamicTextSource::new(move || {
                        resolutions.set(resolutions.get() + 1);
                        Rc::new(crate::text::AnnotatedString::from(text_state.value()))
                    }),
                    Modifier::empty(),
                    TextStyle::default(),
                    TextOverflow::Clip,
                    true,
                    usize::MAX,
                    1,
                );
            }
        })
        .expect("initial text render");

    assert_eq!(resolutions.get(), 1);

    state.set_value("World".to_string());
    composition
        .process_invalid_scopes()
        .expect("dynamic text recomposition");

    assert_eq!(resolutions.get(), 2);
}

fn groups_composed_by(content: impl FnMut() + 'static) -> usize {
    let _app_context = crate::render_state::app_context_test_scope();
    run_test_composition(content)
        .composition
        .debug_slot_table_stats()
        .group_count
}

#[test]
fn every_text_entry_point_composes_as_few_groups_as_the_basic_one() {
    let basic = groups_composed_by(|| {
        BasicTextWithOptions(
            "Hello",
            Modifier::empty(),
            TextStyle::default(),
            TextLayoutOptions::default(),
        );
    });
    let text = groups_composed_by(|| {
        Text("Hello", Modifier::empty(), TextStyle::default());
    });
    let with_options = groups_composed_by(|| {
        TextWithOptions(
            "Hello",
            Modifier::empty(),
            TextStyle::default(),
            TextOptions::default(),
        );
    });
    let basic_text = groups_composed_by(|| {
        BasicText(
            "Hello",
            Modifier::empty(),
            TextStyle::default(),
            TextOverflow::default(),
            true,
            usize::MAX,
            1,
        );
    });
    assert_eq!(
        (text, with_options, basic_text),
        (basic, basic, basic),
        "a wrapper composing through another text composable pays for its group, \
         its read observation and its snapshot on every recomposition"
    );
}
