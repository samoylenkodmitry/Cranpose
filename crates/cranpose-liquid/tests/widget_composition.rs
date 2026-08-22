//! Every Liquid widget, composed.
//!
//! A widget library's unit tests can only reach the arithmetic behind a
//! component — a spec's clamping, a lens's geometry — and those are already
//! covered where they live. What they cannot reach is whether the component
//! composes at all: a `#[composable]` that panics on a missing local, reads a
//! theme that was never provided, or remembers state under a key it shares with
//! a sibling fails only when something actually runs it.
//!
//! So each test here runs one. `run_test_composition` builds an in-memory
//! composition, renders the content once, and hands back the root; a widget that
//! cannot be composed never reaches the assertion.

use cranpose_liquid::prelude::*;
use cranpose_ui::run_test_composition;
use cranpose_ui::Modifier;
use cranpose_ui_graphics::Color;
use std::cell::Cell;
use std::rc::Rc;

/// Runs `content` inside a Liquid theme.
///
/// Nothing is asserted about the tree: content that only reads the theme or
/// remembers a value emits no layout node, and demanding one of it would be
/// asserting the harness rather than the library.
fn in_theme(content: impl FnMut() + 'static) {
    let mut content = content;
    run_test_composition(move || {
        LiquidTheme(LiquidThemeSpec::default(), &mut content);
    });
}

/// Composes `content` inside a Liquid theme and asserts it produced a tree.
fn themed(content: impl FnMut() + 'static) {
    let mut content = content;
    let composition = run_test_composition(move || {
        LiquidTheme(LiquidThemeSpec::default(), &mut content);
    });
    assert!(
        composition.root().is_some(),
        "the composition produced no root node"
    );
}

#[test]
fn a_glass_surface_composes_its_content() {
    let drawn = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&drawn);
    themed(move || {
        GlassSurface(Modifier::empty(), Glass::default(), {
            let counter = Rc::clone(&counter);
            move || counter.set(counter.get() + 1)
        });
    });
    assert_eq!(drawn.get(), 1, "the surface did not compose its content");
}

#[test]
fn a_glass_button_label_composes_inside_a_button_spec() {
    themed(|| {
        GlassButtonLabel("Add", GlassButtonSpec::prominent());
    });
}

#[test]
fn a_glass_icon_button_composes_at_its_diameter() {
    themed(|| {
        GlassIconButton(
            Modifier::empty(),
            GlassButtonSpec::glass(),
            44.0,
            || {},
            cranpose_liquid::icons::SEARCH,
        );
    });
}

#[test]
fn a_glass_icon_button_group_composes_each_action() {
    let actions = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&actions);
    themed(move || {
        GlassIconButtonGroup(
            Modifier::empty(),
            GlassIconButtonGroupSpec::default(),
            |scope| {
                for _ in 0..3 {
                    counter.set(counter.get() + 1);
                    scope.action(cranpose_liquid::icons::SEARCH, "Search", || {});
                }
            },
        );
    });
    assert_eq!(actions.get(), 3, "the group did not run its content scope");
}

#[test]
fn an_icon_composes_at_a_size_and_colour() {
    themed(|| {
        cranpose_liquid::icons::Icon(cranpose_liquid::icons::SEARCH, 24.0, Color::WHITE);
    });
}

#[test]
fn a_card_composes_its_content() {
    let inner = Rc::new(Cell::new(false));
    let flag = Rc::clone(&inner);
    themed(move || {
        LiquidCard(Modifier::empty(), {
            let flag = Rc::clone(&flag);
            move || flag.set(true)
        });
    });
    assert!(inner.get(), "the card did not compose its content");
}

#[test]
fn a_chip_composes_selected_and_unselected() {
    for selected in [false, true] {
        themed(move || {
            LiquidChip(Modifier::empty(), selected, || {}, "Filter");
        });
    }
}

#[test]
fn a_list_section_composes_its_header_and_rows() {
    let rows = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&rows);
    themed(move || {
        LiquidListSection(Modifier::empty(), "Section", {
            let counter = Rc::clone(&counter);
            move || {
                counter.set(counter.get() + 1);
                LiquidListRow(
                    Modifier::empty(),
                    LiquidListRowSpec::default(),
                    || {},
                    || {},
                );
            }
        });
    });
    assert_eq!(rows.get(), 1, "the section did not compose its rows");
}

#[test]
fn a_nav_bar_composes_both_of_its_slots() {
    let slots = Rc::new(Cell::new(0usize));
    let leading = Rc::clone(&slots);
    let trailing = Rc::clone(&slots);
    themed(move || {
        let scroll = cranpose_core::remember(|| cranpose_ui::scroll::ScrollState::new(0.0))
            .with(Clone::clone);
        LiquidNavBar(
            Modifier::empty(),
            LiquidNavBarSpec::new("Title"),
            scroll,
            {
                let leading = Rc::clone(&leading);
                move || leading.set(leading.get() + 1)
            },
            {
                let trailing = Rc::clone(&trailing);
                move || trailing.set(trailing.get() + 1)
            },
        );
    });
    assert_eq!(slots.get(), 2, "a nav bar slot did not compose");
}

#[test]
fn a_segmented_control_composes_every_segment() {
    let segments = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&segments);
    themed(move || {
        LiquidSegmentedControl(
            Modifier::empty(),
            1,
            |_| {},
            |scope| {
                for label in ["One", "Two", "Three"] {
                    counter.set(counter.get() + 1);
                    scope.segment(label);
                }
            },
        );
    });
    assert_eq!(
        segments.get(),
        3,
        "the control did not run its content scope"
    );
}

#[test]
fn a_slider_composes_across_its_range() {
    for value in [0.0, 0.5, 1.0] {
        themed(move || {
            LiquidSlider(Modifier::empty(), value, |_| {});
        });
    }
}

#[test]
fn a_toggle_composes_checked_and_unchecked() {
    for checked in [false, true] {
        themed(move || {
            LiquidToggle(Modifier::empty(), checked, |_| {});
        });
    }
}

#[test]
fn a_tab_bar_with_an_accessory_composes_both() {
    let accessory = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&accessory);
    themed(move || {
        LiquidTabBarWithAccessory(
            Modifier::empty(),
            LiquidTabBarSpec::default(),
            0,
            |_| {},
            |scope| {
                scope.tab(cranpose_liquid::icons::SEARCH, "Search");
                scope.tab(cranpose_liquid::icons::SEARCH, "Browse");
            },
            {
                let counter = Rc::clone(&counter);
                move || {
                    counter.set(counter.get() + 1);
                    LiquidTabBarSearchAccessory(|| {});
                }
            },
        );
    });
    assert_eq!(accessory.get(), 1, "the accessory slot did not compose");
}

#[test]
fn a_menu_icon_button_composes_covered_and_uncovered() {
    for covered in [false, true] {
        themed(move || {
            let gesture = rememberLiquidMenuGesture();
            LiquidMenuIconButton(
                Modifier::empty(),
                GlassButtonSpec::glass(),
                44.0,
                covered,
                gesture,
                || {},
                cranpose_liquid::icons::SEARCH,
            );
        });
    }
}

#[test]
fn an_absorbed_menu_icon_button_composes_transferred_and_not() {
    for transferred in [false, true] {
        themed(move || {
            LiquidMenuAbsorbedIconButton(
                Modifier::empty(),
                GlassButtonSpec::glass(),
                44.0,
                transferred,
                || {},
                cranpose_liquid::icons::SEARCH,
            );
        });
    }
}

#[test]
fn a_menu_trigger_modifier_is_built_in_composition() {
    in_theme(|| {
        let gesture = rememberLiquidMenuGesture();
        let _ = liquid_menu_trigger_input(Modifier::empty(), gesture, || {});
    });
}

#[test]
fn a_menu_gesture_reports_no_press_point_until_it_is_pressed() {
    in_theme(|| {
        let gesture = rememberLiquidMenuGesture();
        assert!(
            gesture.press_point().is_none(),
            "a gesture nobody touched reported a press point"
        );
    });
}

#[test]
fn press_scale_hands_back_a_modifier_and_two_observable_values() {
    in_theme(|| {
        let interaction = cranpose_ui::rememberMutableInteractionSource();
        let (_modifier, pressed, scale) = liquid_press_scale(Modifier::empty(), interaction, 1.12);
        assert!(!pressed.get(), "nothing was pressed yet");
        assert_eq!(scale.get(), 1.0, "the resting scale is unit");
    });
}

#[test]
fn dynamics_are_remembered_once_per_composition() {
    let observed: Rc<std::cell::RefCell<Vec<*const LiquidDynamics>>> = Rc::default();
    let sink = Rc::clone(&observed);
    in_theme(move || {
        sink.borrow_mut()
            .push(Rc::as_ptr(&rememberLiquidDynamics()));
        sink.borrow_mut()
            .push(Rc::as_ptr(&rememberLiquidDynamics()));
    });
    let seen = observed.borrow();
    assert_eq!(seen.len(), 2);
    assert_ne!(
        seen[0], seen[1],
        "two call sites must remember two independent instances"
    );
}

#[test]
fn the_theme_answers_colours_and_typography_inside_composition() {
    in_theme(|| {
        let colors = liquid_colors();
        let typography = liquid_typography();
        assert!(
            !typography.body.span_style.font_size.is_unspecified(),
            "the body style has no size"
        );
        // A scheme has to distinguish its surface from what sits on it, or
        // every label the library draws is invisible.
        assert_ne!(
            colors.label, colors.surface,
            "the scheme cannot tell content from surface"
        );
    });
}

#[test]
fn a_button_spec_resolves_its_icon_colour_against_the_scheme() {
    in_theme(|| {
        let colors = liquid_colors();
        let plain = GlassButtonSpec::glass().icon_color(&colors);
        let tinted = GlassButtonSpec::glass()
            .with_content_color(Color::RED)
            .icon_color(&colors);
        assert_eq!(tinted, Color::RED, "an explicit content colour must win");
        assert_ne!(
            plain, tinted,
            "the scheme's colour and an explicit one must differ"
        );
    });
}

#[test]
fn a_button_spec_carries_the_glass_it_is_given() {
    let spec = GlassButtonSpec::glass().with_glass(Glass::default());
    let plain = GlassButtonSpec::glass();
    assert_ne!(
        format!("{spec:?}"),
        format!("{plain:?}"),
        "with_glass did not record the material"
    );
}
