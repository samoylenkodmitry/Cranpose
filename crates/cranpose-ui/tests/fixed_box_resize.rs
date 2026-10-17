//! A box whose constraints fix its size keeps its measurement when only a
//! child at its top start changes size, and measures again for any other
//! child. A child's parent data the box does not read, as its fill fraction,
//! changes nothing for it. Each frame lays out as a fresh composition of the
//! same state does.

use std::{cell::Cell, rc::Rc};

use cranpose_ui::{
    Alignment, Box, BoxSpec, Column, ColumnSpec, LayoutDirection, LayoutTree, Modifier,
    ProvideLayoutDirection, Size,
};

use crate::{directional_layout::tagged, text_measured_layout_integration::layout_composition};

const VIEWPORT: Size = Size {
    width: 200.0,
    height: 100.0,
};

/// How the bar's width follows the state: points, or a fraction of the track.
#[derive(Clone, Copy, Debug)]
enum Bar {
    Width,
    Fraction,
}

impl Bar {
    fn modifier(self, value: f32) -> Modifier {
        match self {
            Self::Width => Modifier::empty().size_points(value, 10.0),
            Self::Fraction => Modifier::empty()
                .fill_max_width_fraction(value / 100.0)
                .height(10.0),
        }
    }
}

fn track(direction: LayoutDirection, alignment: Alignment, bar: Bar, width: f32) {
    ProvideLayoutDirection(direction, move || {
        Column(
            Modifier::empty().fill_max_size(),
            ColumnSpec::default(),
            move || {
                Box(
                    Modifier::empty()
                        .size_points(100.0, 40.0)
                        .content_description("track"),
                    BoxSpec::default().content_alignment(alignment),
                    move || {
                        Box(
                            bar.modifier(width).content_description("bar"),
                            BoxSpec::default(),
                            || {},
                        );
                    },
                );
            },
        );
    });
}

fn rect(layout: &LayoutTree, tag: &str) -> (f32, f32, f32, f32) {
    let rect = tagged(layout.root(), tag).rect;
    (rect.x, rect.y, rect.width, rect.height)
}

fn resizes_as_a_fresh_layout(direction: LayoutDirection, alignment: Alignment, bar: Bar) {
    let state = Rc::new(Cell::new(None));
    let slot = Rc::clone(&state);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let width = cranpose_core::rememberMutableStateOf(|| 20.0_f32);
        slot.set(Some(width));
        track(direction, alignment, bar, width.get());
    });
    layout_composition(&mut composition, VIEWPORT);
    let width = state.get().expect("width state");
    for value in [60.0, 35.0, 100.0, 80.0, 20.0] {
        width.set(value);
        while composition.process_invalid_scopes().expect("recompose") {}
        let layout = layout_composition(&mut composition, VIEWPORT);
        let mut fresh =
            cranpose_ui::run_test_composition(move || track(direction, alignment, bar, value));
        let fresh = layout_composition(&mut fresh, VIEWPORT);
        for tag in ["track", "bar"] {
            assert_eq!(
                rect(&layout, tag),
                rect(&fresh, tag),
                "{tag} at {value} by {bar:?} in {direction:?} aligned {alignment:?}"
            );
        }
    }
}

#[test]
fn a_child_at_the_top_start_of_a_fixed_box_resizes_as_a_fresh_layout_does() {
    for bar in [Bar::Width, Bar::Fraction] {
        resizes_as_a_fresh_layout(LayoutDirection::Ltr, Alignment::TOP_START, bar);
    }
}

#[test]
fn a_child_of_a_fixed_box_that_aligns_itself_otherwise_moves_as_a_fresh_layout_does() {
    let aligned = |alignment: Alignment| {
        Column(
            Modifier::empty().fill_max_size(),
            ColumnSpec::default(),
            move || {
                Box(
                    Modifier::empty()
                        .size_points(100.0, 40.0)
                        .content_description("track"),
                    BoxSpec::default(),
                    move || {
                        Box(
                            Modifier::empty()
                                .size_points(30.0, 10.0)
                                .align(alignment)
                                .content_description("bar"),
                            BoxSpec::default(),
                            || {},
                        );
                    },
                );
            },
        );
    };
    let state = Rc::new(Cell::new(None));
    let slot = Rc::clone(&state);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let alignment = cranpose_core::rememberMutableStateOf(|| Alignment::TOP_START);
        slot.set(Some(alignment));
        aligned(alignment.get());
    });
    layout_composition(&mut composition, VIEWPORT);
    let alignment = state.get().expect("alignment state");
    for value in [
        Alignment::CENTER,
        Alignment::BOTTOM_END,
        Alignment::TOP_START,
    ] {
        alignment.set(value);
        while composition.process_invalid_scopes().expect("recompose") {}
        let layout = layout_composition(&mut composition, VIEWPORT);
        let mut fresh = cranpose_ui::run_test_composition(move || aligned(value));
        let fresh = layout_composition(&mut fresh, VIEWPORT);
        assert_eq!(
            rect(&layout, "bar"),
            rect(&fresh, "bar"),
            "aligned {value:?}"
        );
    }
}

#[test]
fn children_a_fixed_box_places_by_their_size_resize_as_a_fresh_layout_does() {
    for direction in [LayoutDirection::Ltr, LayoutDirection::Rtl] {
        for alignment in [
            Alignment::TOP_START,
            Alignment::CENTER,
            Alignment::TOP_END,
            Alignment::BOTTOM_START,
        ] {
            for bar in [Bar::Width, Bar::Fraction] {
                resizes_as_a_fresh_layout(direction, alignment, bar);
            }
        }
    }
}
