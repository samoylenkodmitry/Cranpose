//! Which way the interface reads.
//!
//! Layout direction is what turns "start" and "end" into "left" and "right".
//! It is a composition local rather than a global so a screen can pin a
//! direction — a code block, a phone number, a language picker — without
//! reversing the rest of the interface with it.

use cranpose_core::{compositionLocalOf, CompositionLocal, CompositionLocalProvider};

/// Which side of the interface is the start.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LayoutDirection {
    /// Start is the left edge — Latin, Cyrillic, CJK.
    #[default]
    Ltr,
    /// Start is the right edge — Arabic, Hebrew.
    Rtl,
}

impl LayoutDirection {
    /// Whether the start edge is the right one.
    pub fn is_rtl(self) -> bool {
        matches!(self, LayoutDirection::Rtl)
    }

    /// The direction that reads the other way.
    pub fn reversed(self) -> Self {
        match self {
            LayoutDirection::Ltr => LayoutDirection::Rtl,
            LayoutDirection::Rtl => LayoutDirection::Ltr,
        }
    }

    /// Turns start/end values into physical left/right values.
    pub fn resolve(self, start: f32, end: f32) -> (f32, f32) {
        match self {
            LayoutDirection::Ltr => (start, end),
            LayoutDirection::Rtl => (end, start),
        }
    }
}

/// The [`CompositionLocal`] carrying the current layout direction.
pub fn local_layout_direction() -> CompositionLocal<LayoutDirection> {
    thread_local! {
        static LOCAL: std::cell::RefCell<Option<CompositionLocal<LayoutDirection>>> =
            const { std::cell::RefCell::new(None) };
    }
    LOCAL.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| compositionLocalOf(LayoutDirection::default))
            .clone()
    })
}

/// The layout direction in force here.
pub fn layout_direction() -> LayoutDirection {
    local_layout_direction().current()
}

/// Runs `content` in `direction`.
#[allow(non_snake_case)]
pub fn ProvideLayoutDirection(direction: LayoutDirection, content: impl FnOnce()) {
    CompositionLocalProvider(vec![local_layout_direction().provides(direction)], content);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolving_start_and_end_follows_the_direction() {
        assert_eq!(LayoutDirection::Ltr.resolve(4.0, 12.0), (4.0, 12.0));
        assert_eq!(LayoutDirection::Rtl.resolve(4.0, 12.0), (12.0, 4.0));
    }

    #[test]
    fn a_direction_knows_its_opposite() {
        assert_eq!(LayoutDirection::Ltr.reversed(), LayoutDirection::Rtl);
        assert!(!LayoutDirection::Ltr.is_rtl());
        assert!(LayoutDirection::Rtl.is_rtl());
    }

    #[test]
    fn a_provided_direction_reaches_the_content_and_ends_with_it() {
        use std::{cell::Cell, rc::Rc};

        use cranpose_core::{location_key, Composition, MemoryApplier};

        let mut composition = Composition::new(MemoryApplier::new());
        let outer = Rc::new(Cell::new(LayoutDirection::Rtl));
        let inside = Rc::new(Cell::new(LayoutDirection::Ltr));
        let nested = Rc::new(Cell::new(LayoutDirection::Rtl));
        let after = Rc::new(Cell::new(LayoutDirection::Rtl));

        let key = location_key(file!(), line!(), column!());
        {
            let (outer, inside, nested, after) = (
                Rc::clone(&outer),
                Rc::clone(&inside),
                Rc::clone(&nested),
                Rc::clone(&after),
            );
            let mut render = move || {
                // Nothing provided: the interface reads the default way.
                outer.set(layout_direction());
                ProvideLayoutDirection(LayoutDirection::Rtl, || {
                    inside.set(layout_direction());
                    // A screen can pin a direction back the other way without
                    // reversing what surrounds it.
                    ProvideLayoutDirection(LayoutDirection::Ltr, || nested.set(layout_direction()));
                });
                // The provision is scoped to the content, not to what follows.
                after.set(layout_direction());
            };
            composition.render(key, &mut render).expect("render");
        }

        assert_eq!(outer.get(), LayoutDirection::Ltr);
        assert_eq!(inside.get(), LayoutDirection::Rtl);
        assert_eq!(nested.get(), LayoutDirection::Ltr);
        assert_eq!(after.get(), LayoutDirection::Ltr);
    }

    #[test]
    fn the_composition_local_is_one_instance_per_thread() {
        use std::{cell::Cell, rc::Rc};

        use cranpose_core::{location_key, Composition, MemoryApplier};

        // Two calls that returned different locals would each carry their own
        // value, and a provision made through one would be invisible to the
        // other -- which is what a lazily-created local gets wrong.
        let mut composition = Composition::new(MemoryApplier::new());
        let seen = Rc::new(Cell::new(LayoutDirection::Ltr));
        let recorder = Rc::clone(&seen);
        let key = location_key(file!(), line!(), column!());
        let mut render = move || {
            CompositionLocalProvider(
                vec![local_layout_direction().provides(LayoutDirection::Rtl)],
                || recorder.set(local_layout_direction().current()),
            );
        };
        composition.render(key, &mut render).expect("render");

        assert_eq!(seen.get(), LayoutDirection::Rtl);
    }
}
