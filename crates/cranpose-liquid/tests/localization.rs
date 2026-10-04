use std::{cell::Cell, rc::Rc};

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_foundation::text::TextFieldState;
use cranpose_liquid::prelude::*;
use cranpose_testing::{create_headless_robot_test, placed_semantics_from_shell};
use cranpose_ui::{Catalog, Column, ColumnSpec, Locale, Modifier, ProvideLocalization, tr};

#[test]
fn changing_language_updates_tab_search_and_action_accessibility_labels() {
    let state_capture = Rc::new(Cell::new(None::<MutableState<Locale>>));
    let capture = state_capture.clone();
    let catalog = Catalog::from_resources(
        "en",
        &[cranpose_ui::localization::Resource {
            locale: "fr",
            namespace: "cranpose-liquid",
            source: "home = Accueil\nsave = Enregistrer",
        }],
    )
    .expect("catalog");
    let mut robot = create_headless_robot_test(440, 400, move || {
        let language = rememberMutableStateOf(|| Locale::parse("en").expect("locale"));
        capture.set(Some(language));
        ProvideLocalization(&catalog, language.value(), || {
            LiquidTheme(LiquidThemeSpec::default(), || {
                Column(
                    Modifier::empty().fill_max_size(),
                    ColumnSpec::default(),
                    || {
                        LiquidSearchField(
                            Modifier::empty().width(320.0),
                            cranpose_core::remember(|| TextFieldState::new(""))
                                .with(|state| *state),
                            LiquidSearchFieldSpec::default(),
                        );
                        GlassButton(
                            Modifier::empty(),
                            GlassButtonSpec::glass(),
                            || {},
                            || {
                                GlassButtonLabel(
                                    tr!("Save", id = "save"),
                                    GlassButtonSpec::glass(),
                                );
                            },
                        );
                        LiquidTabBar(
                            Modifier::empty().width(320.0),
                            LiquidTabBarSpec::default(),
                            0,
                            |_| {},
                            |scope| {
                                scope.tab(cranpose_liquid::icons::SEARCH, tr!("Home", id = "home"));
                            },
                        );
                    },
                );
            });
        });
    });
    robot.shell_mut().set_semantics_enabled(true);
    for (language, expected) in [
        ("en", ["Home", "Save", "Search"]),
        ("fr", ["Accueil", "Enregistrer", "Rechercher"]),
    ] {
        state_capture
            .get()
            .expect("language state")
            .set_value(Locale::parse(language).expect("locale"));
        robot.wait_for_idle();
        let tree = placed_semantics_from_shell(robot.shell_mut()).expect("placed localized UI");
        let nodes = tree.flatten();
        for label in expected {
            assert!(
                nodes
                    .iter()
                    .any(|node| node.label.as_deref() == Some(label)),
                "missing accessible label {label}"
            );
        }
    }
}
