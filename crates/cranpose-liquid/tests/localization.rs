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

#[test]
fn rtl_tabs_remain_visible_and_select_the_logical_index() {
    let selected = Rc::new(Cell::new(usize::MAX));
    let selection = selected.clone();
    let mut robot = create_headless_robot_test(400, 120, move || {
        let selection = selection.clone();
        cranpose_ui::ProvideLayoutDirection(cranpose_ui::LayoutDirection::Rtl, || {
            LiquidTheme(LiquidThemeSpec::default(), move || {
                LiquidTabBar(
                    Modifier::empty().width(390.0),
                    LiquidTabBarSpec::default(),
                    2,
                    move |index| selection.set(index),
                    |tabs| {
                        tabs.tab(cranpose_liquid::icons::BOOKMARK, "Library");
                        tabs.tab(cranpose_liquid::icons::SEARCH, "Scan");
                        tabs.tab(cranpose_liquid::icons::STAR, "Settings");
                    },
                );
            });
        });
    });
    robot.wait_for_idle();
    let mut previous = 400.0;
    for (index, label) in ["Library", "Scan", "Settings"].into_iter().enumerate() {
        let bounds = robot.find_by_text(label).bounds().expect("tab bounds");
        assert!(
            bounds.x >= 0.0 && bounds.x + bounds.width <= 400.0,
            "{label}: {bounds:?}"
        );
        assert!(bounds.x < previous, "tabs follow right-to-left order");
        previous = bounds.x;
        robot.find_by_text(label).click();
        assert_eq!(selected.get(), index, "{label} selects its logical index");
    }
}
