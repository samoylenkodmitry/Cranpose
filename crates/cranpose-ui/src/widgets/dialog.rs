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

use std::rc::Rc;

use cranpose_ui_graphics::{Brush, Color, Point, Rect, Size};
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
    /// Where the dialog's surface sits in the window: the centre, or the
    /// bottom edge for a sheet.
    pub alignment: Alignment,
}

impl Default for DialogSpec {
    fn default() -> Self {
        Self {
            dismiss_on_outside_tap: true,
            dismiss_on_back: true,
            apply_window_insets: true,
            alignment: Alignment::CENTER,
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
            alignment: Alignment::CENTER,
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

    /// Sets where the dialog's surface sits in the window.
    pub const fn with_alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
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
    DialogWithFadingScrim(spec, scrim, || 1.0, on_dismiss, content);
}

/// A modal dialog whose scrim shows `scrim` at `scrim_alpha()` of its
/// alpha. `scrim_alpha` is read while the scrim draws, so a scrim that fades
/// with an animation, as a sheet's does, redraws without recomposing.
#[composable]
pub fn DialogWithFadingScrim<C>(
    spec: DialogSpec,
    scrim: Color,
    scrim_alpha: impl Fn() -> f32 + 'static,
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
    cranpose_core::DisposableEffect((), move |scope| {
        let registration = crate::modal::register_modal(Rc::new(move || (back.value())()));
        scope.on_dispose(move || drop(registration))
    });

    let scrim_alpha = Rc::new(scrim_alpha);
    PopupDismissable(bounds, Point { x: 0.0, y: 0.0 }, scrim_dismiss, move || {
        let content = Rc::clone(&content);
        let scrim_alpha = Rc::clone(&scrim_alpha);
        Box(
            Modifier::empty()
                .size(Size {
                    width: bounds.width,
                    height: bounds.height,
                })
                .draw_behind(move |scope| {
                    let alpha = scrim.3 * scrim_alpha().clamp(0.0, 1.0);
                    scope.draw_rect(Brush::solid(Color(scrim.0, scrim.1, scrim.2, alpha)));
                })
                .focus_target()
                .stable_semantics(move |config| {
                    config.role = Some(SemanticsWidgetRole::Dialog);
                    config.is_modal = true;
                    // A reader stays inside the dialog, so the dialog itself
                    // offers the way out that the back gesture takes.
                    config.dismiss = dismiss_on_back.then(|| {
                        cranpose_foundation::SemanticsDismiss::new(move || {
                            (back.value())();
                            true
                        })
                    });
                }),
            BoxSpec::default().content_alignment(spec.alignment),
            move || {
                let content = Rc::clone(&content);
                // Taps on the dialog's own surface stay off the scrim below.
                Box(
                    Modifier::empty().pointer_opaque(),
                    BoxSpec::default(),
                    move || {
                        let content = Rc::clone(&content);
                        cranpose_core::CompositionLocalProvider(
                            [crate::modal::local_modal_depth().provides(own_modal_depth)],
                            move || content(),
                        );
                    },
                );
            },
        );
    });
}

#[cfg(test)]
#[path = "tests/dialog_tests.rs"]
mod tests;
