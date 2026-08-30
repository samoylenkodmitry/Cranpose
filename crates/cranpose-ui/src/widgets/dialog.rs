//! Modal dialogs.
//!
//! A dialog is the one surface that takes the whole screen's attention: while
//! it is open, everything behind it is inert, a back gesture closes it rather
//! than the screen behind it, a screen reader announces it and stays inside it,
//! and a text field inside it keeps its focus while everything outside loses
//! the ability to take focus away.
//!
//! Dialogs are built on [`crate::widgets::popup`], so their content escapes
//! every ancestor clip and draws above the whole application, and they are
//! centred inside the window insets so the system bars and the on-screen
//! keyboard never cover them.

#![allow(non_snake_case)]

use std::rc::Rc;

use cranpose_ui_graphics::{Color, Point, Rect, Size};
use cranpose_ui_layout::Alignment;

use super::{
    box_widget::{Box, BoxSpec},
    popup::{PopupDismissable, local_popup_viewport},
};
use crate::{Modifier, SemanticsWidgetRole, composable, safe_area::window_insets};

/// Why a dialog is being asked to close.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DismissReason {
    /// The user tapped outside the dialog's surface.
    OutsideTap,
    /// The user made the platform's back gesture, or pressed Escape.
    BackRequested,
}

/// How a dialog behaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DialogSpec {
    /// Whether a tap outside the dialog dismisses it.
    pub dismiss_on_outside_tap: bool,
    /// Whether the platform's back gesture dismisses it.
    pub dismiss_on_back: bool,
    /// Whether the dialog keeps clear of the system bars and the on-screen
    /// keyboard. A dialog that draws its own full-bleed surface turns this off.
    pub apply_window_insets: bool,
}

impl Default for DialogSpec {
    fn default() -> Self {
        Self {
            dismiss_on_outside_tap: true,
            dismiss_on_back: true,
            apply_window_insets: true,
        }
    }
}

impl DialogSpec {
    /// A dialog the user must answer: neither an outside tap nor a back
    /// gesture closes it, so only the dialog's own controls can.
    pub const fn required() -> Self {
        Self {
            dismiss_on_outside_tap: false,
            dismiss_on_back: false,
            apply_window_insets: true,
        }
    }

    /// Sets whether an outside tap dismisses the dialog.
    pub const fn with_dismiss_on_outside_tap(mut self, dismiss: bool) -> Self {
        self.dismiss_on_outside_tap = dismiss;
        self
    }

    /// Sets whether the back gesture dismisses the dialog.
    pub const fn with_dismiss_on_back(mut self, dismiss: bool) -> Self {
        self.dismiss_on_back = dismiss;
        self
    }

    /// Sets whether the dialog keeps clear of system insets.
    pub const fn with_window_insets(mut self, apply: bool) -> Self {
        self.apply_window_insets = apply;
        self
    }
}

/// The default dim behind a dialog, matching what every platform's own modal
/// surfaces use closely enough that a dialog does not look foreign.
pub const DEFAULT_SCRIM: Color = Color(0.0, 0.0, 0.0, 0.4);

/// The rectangle a dialog's surface is placed in.
///
/// The dialog fills the window minus its insets; the surface itself is centred
/// inside that and sized by its own content.
pub(crate) fn dialog_bounds(viewport: Size, insets: cranpose_ui_graphics::EdgeInsets) -> Rect {
    let width = (viewport.width - insets.left - insets.right).max(0.0);
    let height = (viewport.height - insets.top - insets.bottom).max(0.0);
    Rect::from_origin_size(
        Point {
            x: insets.left,
            y: insets.top,
        },
        Size { width, height },
    )
}

/// A modal dialog holding `content`, dismissed through `on_dismiss`.
///
/// `on_dismiss` receives why the dialog is closing, so an application can treat
/// "tapped away" differently from "went back" — a draft, for example, is
/// usually kept in one case and discarded in the other.
///
/// ```rust,no_run
/// # use cranpose_ui::widgets::{Dialog, DialogSpec};
/// # use cranpose_ui::Modifier;
/// # fn Content() {}
/// # let open = true;
/// if open {
///     Dialog(
///         DialogSpec::default(),
///         |_reason| { /* close it */ },
///         || {
///             Content();
///         },
///     );
/// }
/// ```
#[composable]
pub fn Dialog<C>(spec: DialogSpec, on_dismiss: impl Fn(DismissReason) + 'static, content: C)
where
    C: Fn() + 'static,
{
    DialogWithScrim(spec, DEFAULT_SCRIM, on_dismiss, content);
}

/// A modal dialog whose scrim colour is chosen by the caller.
#[composable]
pub fn DialogWithScrim<C>(
    spec: DialogSpec,
    scrim: Color,
    on_dismiss: impl Fn(DismissReason) + 'static,
    content: C,
) where
    C: Fn() + 'static,
{
    let own_modal_depth =
        cranpose_core::remember(|| crate::modal::current_modal_depth() + 1).with(|depth| *depth);

    let viewport = local_popup_viewport().current().get();
    let insets = if spec.apply_window_insets {
        window_insets().combined()
    } else {
        cranpose_ui_graphics::EdgeInsets::default()
    };
    let bounds = dialog_bounds(viewport, insets);

    let on_dismiss = Rc::new(on_dismiss);
    let content = Rc::new(content);

    let dismiss_on_outside_tap = spec.dismiss_on_outside_tap;
    let scrim_dismiss = {
        let on_dismiss = Rc::clone(&on_dismiss);
        move || {
            if dismiss_on_outside_tap {
                on_dismiss(DismissReason::OutsideTap);
            }
        }
    };

    let dismiss_on_back = spec.dismiss_on_back;
    let back = {
        let on_dismiss = Rc::clone(&on_dismiss);
        cranpose_core::rememberUpdatedState::<Rc<dyn Fn()>>(Rc::new(move || {
            if dismiss_on_back {
                on_dismiss(DismissReason::BackRequested);
            }
        }))
    };
    cranpose_core::DisposableEffect!((), move |scope| {
        let registration = crate::modal::register_modal(Rc::new(move || (back.value())()));
        scope.on_dispose(move || drop(registration))
    });

    PopupDismissable(bounds, Point { x: 0.0, y: 0.0 }, scrim_dismiss, move || {
        let content = Rc::clone(&content);
        Box(
            Modifier::empty()
                .size(Size {
                    width: bounds.width,
                    height: bounds.height,
                })
                .background(scrim)
                .semantics(move |config| {
                    config.role = Some(SemanticsWidgetRole::Dialog);
                    config.is_modal = true;
                }),
            BoxSpec::default().content_alignment(Alignment::CENTER),
            move || {
                let content = Rc::clone(&content);
                cranpose_core::CompositionLocalProvider(
                    [crate::modal::local_modal_depth().provides(own_modal_depth)],
                    move || content(),
                );
            },
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dialog_fills_the_window_inside_its_insets() {
        let viewport = Size {
            width: 400.0,
            height: 800.0,
        };
        let insets = cranpose_ui_graphics::EdgeInsets::from_components(8.0, 24.0, 8.0, 48.0);
        let bounds = dialog_bounds(viewport, insets);
        assert_eq!(bounds.x, 8.0);
        assert_eq!(bounds.y, 24.0);
        assert_eq!(bounds.width, 384.0);
        assert_eq!(bounds.height, 728.0);
    }

    #[test]
    fn insets_larger_than_the_window_collapse_rather_than_going_negative() {
        let viewport = Size {
            width: 100.0,
            height: 100.0,
        };
        let insets = cranpose_ui_graphics::EdgeInsets::from_components(80.0, 80.0, 80.0, 80.0);
        let bounds = dialog_bounds(viewport, insets);
        assert_eq!(bounds.width, 0.0);
        assert_eq!(bounds.height, 0.0);
    }

    #[test]
    fn a_required_dialog_refuses_both_dismissals_but_still_covers_the_screen() {
        let spec = DialogSpec::required();
        assert!(!spec.dismiss_on_outside_tap);
        assert!(!spec.dismiss_on_back);
        assert!(spec.apply_window_insets);
    }

    #[test]
    fn the_default_dialog_dismisses_both_ways_until_told_otherwise() {
        let spec = DialogSpec::default();
        assert!(spec.dismiss_on_outside_tap);
        assert!(spec.dismiss_on_back);
        assert!(!spec.with_dismiss_on_back(false).dismiss_on_back);
        assert!(
            !spec
                .with_dismiss_on_outside_tap(false)
                .dismiss_on_outside_tap
        );
        assert!(!spec.with_window_insets(false).apply_window_insets);
    }

    #[test]
    fn a_dialog_traps_focus_inside_it_and_releases_it_on_close() {
        use std::cell::{Cell, RefCell};

        use cranpose_core::{
            Composition, MemoryApplier, MutableState, NodeId, location_key, mutableStateOf,
            remember,
        };
        use cranpose_foundation::{PointerEvent, PointerEventKind, text::TextFieldState};
        use cranpose_ui_graphics::Size;

        use crate::{
            layout::{LayoutBox, LayoutEngine, LayoutTree},
            text::TextStyle,
            text_field_focus::{focused_editor_state, has_focused_field},
            widgets::{basic_text_field::BasicTextField, popup::PopupHost},
        };

        let _app_context = crate::render_state::app_context_test_scope();
        crate::modal::clear_modals();

        let mut composition = Composition::new(MemoryApplier::new());

        let outside_state = TextFieldState::new("outside");
        let inside_state = TextFieldState::new("inside");
        let outside_id: Rc<Cell<Option<NodeId>>> = Rc::new(Cell::new(None));
        let inside_id: Rc<Cell<Option<NodeId>>> = Rc::new(Cell::new(None));
        let dialog_open_slot: Rc<RefCell<Option<MutableState<bool>>>> = Rc::new(RefCell::new(None));

        let mut content = {
            let outside_id = Rc::clone(&outside_id);
            let inside_id = Rc::clone(&inside_id);
            let dialog_open_slot = Rc::clone(&dialog_open_slot);
            move || {
                let outside_id = Rc::clone(&outside_id);
                let inside_id = Rc::clone(&inside_id);
                let dialog_open_slot = Rc::clone(&dialog_open_slot);
                PopupHost(move || {
                    let dialog_open = remember(|| mutableStateOf(true)).with(|state| *state);
                    *dialog_open_slot.borrow_mut() = Some(dialog_open);

                    outside_id.set(Some(BasicTextField(
                        outside_state,
                        Modifier::empty(),
                        TextStyle::default(),
                    )));

                    if dialog_open.value() {
                        let inside_id = Rc::clone(&inside_id);
                        Dialog(
                            DialogSpec::default(),
                            |_reason: DismissReason| {},
                            move || {
                                inside_id.set(Some(BasicTextField(
                                    inside_state,
                                    Modifier::empty(),
                                    TextStyle::default(),
                                )));
                            },
                        );
                    }
                });
            }
        };

        let key = location_key(file!(), line!(), column!());
        composition.render(key, &mut content).expect("render");

        let mut settle = move |composition: &mut Composition<MemoryApplier>| -> LayoutTree {
            for _ in 0..16 {
                if !composition.should_render() {
                    break;
                }
                composition.reconcile(key, &mut content).expect("reconcile");
            }
            let root = composition.root().expect("root");
            let handle = composition.runtime_handle();
            let mut applier = composition.applier_mut();
            applier.set_runtime_handle(handle);
            let layout = applier
                .compute_layout(root, Size::new(400.0, 800.0))
                .expect("layout");
            applier.clear_runtime_handle();
            drop(applier);
            layout
        };

        fn find_node(node: &LayoutBox, id: NodeId) -> Option<&LayoutBox> {
            if node.node_id == id {
                return Some(node);
            }
            node.children.iter().find_map(|child| find_node(child, id))
        }

        fn tap(layout_root: &LayoutBox, id: &Rc<Cell<Option<NodeId>>>) {
            let id = id.get().expect("field composed");
            let node = find_node(layout_root, id).expect("field placed in the layout");
            let handler = node
                .node_data
                .modifier_slices()
                .pointer_inputs()
                .first()
                .cloned()
                .expect("field has a pointer handler");
            let position = cranpose_ui_graphics::Point { x: 1.0, y: 1.0 };
            handler(PointerEvent::new(
                PointerEventKind::Down,
                position,
                position,
            ));
        }

        let mut layout = settle(&mut composition);
        for _ in 0..7 {
            layout = settle(&mut composition);
        }

        tap(layout.root(), &inside_id);
        assert!(has_focused_field(), "the dialog's own field must focus");
        assert_eq!(
            focused_editor_state().map(|s| s.text),
            Some("inside".to_string()),
            "the dialog's own field must be the one focused"
        );

        tap(layout.root(), &outside_id);
        assert_eq!(
            focused_editor_state().map(|s| s.text),
            Some("inside".to_string()),
            "a field behind an open dialog must not take focus from it"
        );

        let dialog_open = dialog_open_slot
            .borrow()
            .as_ref()
            .copied()
            .expect("dialog_open captured");
        dialog_open.set(false);
        layout = settle(&mut composition);
        assert!(
            !has_focused_field(),
            "closing the dialog must release focus from the field it held"
        );

        tap(layout.root(), &outside_id);
        assert_eq!(
            focused_editor_state().map(|s| s.text),
            Some("outside".to_string()),
            "once the dialog is closed, the field behind it must be focusable"
        );

        crate::modal::clear_modals();
        crate::text_field_focus::clear_focus();
    }
}
