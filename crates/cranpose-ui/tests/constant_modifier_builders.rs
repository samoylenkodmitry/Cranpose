//! `#[composable]` keeps a modifier chain of constants in a slot and builds
//! it once, so each builder it lets into such a chain must read nothing but
//! its receiver and arguments. Each one builds equal modifiers at another
//! density and in the other layout direction, and the list here is the
//! macro's list.

use std::{cell::RefCell, rc::Rc};

use cranpose_ui::{
    Alignment, Color, Density, HorizontalAlignment, LayoutDirection, Modifier,
    ProvideLayoutDirection, RoundedCornerShape, Size, VerticalAlignment, density::ProvideDensity,
};

const MACRO_SOURCE: &str = include_str!("../../cranpose-macros/src/constant_modifiers.rs");

type Builder = (&'static str, fn() -> Modifier);

fn builders() -> Vec<Builder> {
    vec![
        ("align", || Modifier::empty().align(Alignment::CENTER)),
        ("background", || {
            Modifier::empty().background(Color::from_rgb_u8(40, 44, 64))
        }),
        ("border", || {
            Modifier::empty().border(1.0, Color::WHITE, RoundedCornerShape::uniform(4.0))
        }),
        ("clip_to_bounds", || Modifier::empty().clip_to_bounds()),
        ("fill_max_height", || Modifier::empty().fill_max_height()),
        ("fill_max_height_fraction", || {
            Modifier::empty().fill_max_height_fraction(0.5)
        }),
        ("fill_max_size", || Modifier::empty().fill_max_size()),
        ("fill_max_size_fraction", || {
            Modifier::empty().fill_max_size_fraction(0.5)
        }),
        ("fill_max_width", || Modifier::empty().fill_max_width()),
        ("fill_max_width_fraction", || {
            Modifier::empty().fill_max_width_fraction(0.5)
        }),
        ("height", || Modifier::empty().height(16.0)),
        ("height_in", || Modifier::empty().height_in(4.0, 40.0)),
        ("offset", || Modifier::empty().offset(3.0, -2.0)),
        ("padding", || Modifier::empty().padding(8.0)),
        ("padding_each", || {
            Modifier::empty().padding_each(1.0, 2.0, 3.0, 4.0)
        }),
        ("padding_horizontal", || {
            Modifier::empty().padding_horizontal(6.0)
        }),
        ("padding_symmetric", || {
            Modifier::empty().padding_symmetric(6.0, 2.0)
        }),
        ("padding_vertical", || {
            Modifier::empty().padding_vertical(6.0)
        }),
        ("required_height", || {
            Modifier::empty().required_height(12.0)
        }),
        ("required_size", || {
            Modifier::empty().required_size(Size {
                width: 12.0,
                height: 8.0,
            })
        }),
        ("required_width", || Modifier::empty().required_width(12.0)),
        ("rounded_corner_shape", || {
            Modifier::empty().rounded_corner_shape(RoundedCornerShape::uniform(6.0))
        }),
        ("rounded_corners", || Modifier::empty().rounded_corners(8.0)),
        ("size", || {
            Modifier::empty().size(Size {
                width: 20.0,
                height: 10.0,
            })
        }),
        ("size_in", || {
            Modifier::empty().size_in(
                Size {
                    width: 1.0,
                    height: 1.0,
                },
                Size {
                    width: 30.0,
                    height: 20.0,
                },
            )
        }),
        ("size_points", || Modifier::empty().size_points(20.0, 10.0)),
        ("weight", || Modifier::empty().weight(2.0)),
        ("weight_with_fill", || {
            Modifier::empty().weight_with_fill(2.0, false)
        }),
        ("width", || Modifier::empty().width(24.0)),
        ("width_in", || Modifier::empty().width_in(4.0, 40.0)),
        ("wrap_content_height", || {
            Modifier::empty().wrap_content_height(VerticalAlignment::CenterVertically, false)
        }),
        ("wrap_content_size", || {
            Modifier::empty().wrap_content_size(Alignment::CENTER, true)
        }),
        ("wrap_content_width", || {
            Modifier::empty().wrap_content_width(HorizontalAlignment::End, false)
        }),
    ]
}

/// The names in the macro's `PURE_BUILDERS`.
fn macro_builders() -> Vec<String> {
    let start = MACRO_SOURCE
        .find("PURE_BUILDERS: &[&str] = &[")
        .expect("the macro's builder list");
    let list = &MACRO_SOURCE[start..];
    let end = list.find("];").expect("the end of the builder list");
    list[..end]
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// What each builder builds while composing at `density` in `direction`.
fn built_at(density: f32, direction: LayoutDirection) -> Vec<Modifier> {
    let built = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&built);
    let _composition = cranpose_ui::run_test_composition(move || {
        let sink = Rc::clone(&sink);
        ProvideDensity(Density::new(density, 1.0), move || {
            ProvideLayoutDirection(direction, move || {
                let mut sink = sink.borrow_mut();
                sink.clear();
                sink.extend(builders().into_iter().map(|(_, build)| build()));
            });
        });
    });
    built.take()
}

#[test]
fn every_builder_a_kept_chain_takes_builds_the_same_modifier_anywhere() {
    let mut names: Vec<String> = builders()
        .iter()
        .map(|(name, _)| (*name).to_owned())
        .collect();
    names.sort();
    let mut listed = macro_builders();
    listed.sort();
    assert_eq!(
        names, listed,
        "the builders tested here are the ones the macro keeps"
    );

    let plain = built_at(1.0, LayoutDirection::Ltr);
    let other = built_at(2.0, LayoutDirection::Rtl);
    assert_eq!(plain.len(), names.len());
    for (((name, _), plain), other) in builders().iter().zip(&plain).zip(&other) {
        assert!(
            plain == other,
            "{name} builds another modifier at another density or direction"
        );
    }
}
