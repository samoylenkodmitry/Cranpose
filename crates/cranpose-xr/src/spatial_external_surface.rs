//! A surface that a video decoder or a camera writes into, shown in a subspace.

use std::{cell::RefCell, rc::Rc};

use cranpose::{ExternalSurface, prelude::*};

use crate::{
    StereoMode, SubspaceModifier,
    space::{SUBSPACE_ORIGIN, Space, local_space},
    subspace_modifier::DEFAULT_PANEL_SIZE,
};

type Callback = Box<dyn FnMut(ExternalSurface)>;

// The callbacks that the content registered. The scope's methods set them, and the surface's
// effect calls them.
#[derive(Default)]
struct CallbackSlots {
    created: RefCell<Option<Callback>>,
    destroyed: RefCell<Option<Callback>>,
}

#[derive(Clone, Default)]
struct Callbacks(Rc<CallbackSlots>);

impl PartialEq for Callbacks {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

// Calls the callback in `slot` with `surface`. The callback runs with no borrow held, so it may
// set a new callback.
fn invoke(slot: &RefCell<Option<Callback>>, surface: ExternalSurface) {
    let taken = slot.borrow_mut().take();
    if let Some(mut callback) = taken {
        callback(surface);
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(callback);
        }
    }
}

/// The scope of the content of a [`SpatialExternalSurface`], which receives the surface.
/// Jetpack XR's `SpatialExternalSurfaceScope`.
pub struct SpatialExternalSurfaceScope {
    callbacks: Callbacks,
}

impl SpatialExternalSurfaceScope {
    /// Sets the callback that receives the surface once the headset has made it, on the
    /// thread of the composition. A decoder or a camera writes into the surface from any thread.
    /// Jetpack XR's `onSurfaceCreated`.
    pub fn on_surface_created(&self, callback: impl FnMut(ExternalSurface) + 'static) {
        *self.callbacks.0.created.borrow_mut() = Some(Box::new(callback));
    }

    /// Sets the callback that receives the surface when the composable leaves the composition,
    /// on the thread of the composition. The writer must stop before the callback returns. The
    /// headset ends the surface afterwards. Jetpack XR's `onSurfaceDestroyed`.
    pub fn on_surface_destroyed(&self, callback: impl FnMut(ExternalSurface) + 'static) {
        *self.callbacks.0.destroyed.borrow_mut() = Some(Box::new(callback));
    }
}

/// A surface of the size that `modifier` gives, into which a video decoder or a camera writes
/// pictures. Jetpack XR's `SpatialExternalSurface`.
///
/// In a headset the compositor shows the pictures as a layer at the surface's place, and the
/// app's own composition and the GPU never copy them. `content` receives the scope that
/// delivers the [`ExternalSurface`] through [`on_surface_created`] and
/// [`on_surface_destroyed`]. `stereo_mode` tells how the picture divides between the eyes. A
/// controller's ray stops on the surface, and its trigger does nothing there.
///
/// Outside a headset the surface draws as an empty box of its size, and the callbacks never run.
///
/// [`on_surface_created`]: SpatialExternalSurfaceScope::on_surface_created
/// [`on_surface_destroyed`]: SpatialExternalSurfaceScope::on_surface_destroyed
#[composable]
pub fn SpatialExternalSurface(
    stereo_mode: StereoMode,
    modifier: SubspaceModifier,
    content: impl FnMut(&SpatialExternalSurfaceScope) + 'static,
) {
    let size = modifier.size_or(DEFAULT_PANEL_SIZE);
    match local_space().current() {
        Some(space) => PlacedSurface(space, stereo_mode, modifier, size, content),
        None => {
            Box(
                Modifier::empty().size_points(size[0], size[1]),
                BoxSpec::default(),
                || {},
            );
        }
    }
}

#[composable]
fn PlacedSurface(
    space: Space,
    stereo_mode: StereoMode,
    modifier: SubspaceModifier,
    size: [f32; 2],
    content: impl FnMut(&SpatialExternalSurfaceScope) + 'static,
) {
    let panel = remember({
        let space = space.clone();
        move || space.new_panel()
    })
    .with(|panel| *panel);
    space.place_surface(
        panel,
        stereo_mode,
        size,
        SUBSPACE_ORIGIN.then(&modifier.pose()),
    );
    let id = panel.id;
    space.set_mobility(id, modifier.mobility());
    DisposableEffect(id, move |_| {
        DisposableEffectResult::new(move || space.remove(id))
    });
    let callbacks = remember(Callbacks::default).with(Clone::clone);
    let mut content = content;
    content(&SpatialExternalSurfaceScope {
        callbacks: callbacks.clone(),
    });
    SurfaceEvents(panel.surface, callbacks);
}

// The effect reads the surface's state, so a new surface composes only this function.
#[composable]
fn SurfaceEvents(surface: MutableState<Option<ExternalSurface>>, callbacks: Callbacks) {
    let Some(surface) = surface.get() else {
        return;
    };
    DisposableEffect(surface.clone(), move |_| {
        invoke(&callbacks.0.created, surface.clone());
        DisposableEffectResult::new(move || invoke(&callbacks.0.destroyed, surface))
    });
}
