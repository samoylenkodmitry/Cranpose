//! A surface that a video decoder or a camera writes into, shown in a subspace.

use std::{cell::RefCell, rc::Rc};

use cranpose::{ExternalSurface, PointerEvent, prelude::*};

use crate::{
    StereoMode, SubspaceModifier,
    space::{Space, local_space, local_subspace_origin},
    subspace_modifier::DEFAULT_PANEL_SIZE,
    surface_input::PointerSink,
};

type Callback<Argument> = Box<dyn FnMut(Argument)>;

// The callbacks that the content registered. The scope's methods set them, and the surface's
// effect calls them.
#[derive(Default)]
struct CallbackSlots {
    created: RefCell<Option<Callback<ExternalSurface>>>,
    destroyed: RefCell<Option<Callback<ExternalSurface>>>,
    pointer: RefCell<Option<Callback<PointerEvent>>>,
}

#[derive(Clone, Default)]
struct Callbacks(Rc<CallbackSlots>);

impl PartialEq for Callbacks {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

// Calls the callback in `slot` with `argument`. The callback runs with no borrow held, so it may
// set a new callback.
fn invoke<Argument>(slot: &RefCell<Option<Callback<Argument>>>, argument: Argument) {
    let taken = slot.borrow_mut().take();
    if let Some(mut callback) = taken {
        callback(argument);
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

    /// Sets the callback that receives the pointer events of a controller's ray on the surface, as
    /// Compose's pointer input gives them to a composable. The positions are in dp from the
    /// surface's top left corner, in the units of the surface's size. A surface of `width` by
    /// `height` dp that shows a picture of `w` by `h` pixels maps a position to the picture's
    /// pixel `(x / width * w, y / height * h)`. The events are `Enter` when the ray first meets
    /// the surface, `Move` while it moves over it, `Down` when the trigger goes down and `Up`
    /// when it goes up, and `Exit` when the ray leaves. An `Up` comes also when the ray left the
    /// surface during the press, or the controller was lost, at the last place on the surface.
    /// Jetpack XR's `InteractionPolicy.onInputEvent`.
    pub fn on_pointer_event(&self, callback: impl FnMut(PointerEvent) + 'static) {
        *self.callbacks.0.pointer.borrow_mut() = Some(Box::new(callback));
    }
}

/// A surface of the size that `modifier` gives, into which a video decoder or a camera writes
/// pictures. Jetpack XR's `SpatialExternalSurface`.
///
/// In a headset the compositor shows the pictures as a layer at the surface's place, and the
/// app's own composition and the GPU never copy them. `content` receives the scope that
/// delivers the [`ExternalSurface`] through [`on_surface_created`] and
/// [`on_surface_destroyed`]. `stereo_mode` tells how the picture divides between the eyes. A
/// controller's ray stops on the surface. Its trigger does nothing there, unless the content sets
/// [`on_pointer_event`].
///
/// The modifier's [`resizable`](SubspaceModifier::resizable) lets the user resize the surface. The
/// picture fills the whole surface, so a resize that does not
/// [maintain the aspect ratio](crate::ResizeLimits::maintain_aspect_ratio) stretches it. The
/// surface's buffers keep the size that they had when the surface appeared.
///
/// Outside a headset the surface draws as an empty box of its size, and the callbacks never run.
///
/// [`on_surface_created`]: SpatialExternalSurfaceScope::on_surface_created
/// [`on_surface_destroyed`]: SpatialExternalSurfaceScope::on_surface_destroyed
/// [`on_pointer_event`]: SpatialExternalSurfaceScope::on_pointer_event
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
    let origin = local_subspace_origin().current();
    space.place_surface(
        panel,
        stereo_mode,
        size,
        origin,
        origin.then(&modifier.pose()),
    );
    let id = panel.id;
    space.set_move_policy(id, modifier.move_policy());
    space.set_resizable(id, modifier.resize());
    let callbacks = remember(Callbacks::default).with(Clone::clone);
    let mut content = content;
    content(&SpatialExternalSurfaceScope {
        callbacks: callbacks.clone(),
    });
    // The sink is made once and stays while the content sets a pointer callback. A surface with
    // no callback takes no pointer events.
    let sink = remember({
        let callbacks = callbacks.clone();
        move || {
            let sink: PointerSink = Rc::new(move |event| invoke(&callbacks.0.pointer, event));
            sink
        }
    })
    .with(Clone::clone);
    let listens = callbacks.0.pointer.borrow().is_some();
    space.set_surface_sink(id, listens.then_some(sink));
    DisposableEffect(id, move |_| {
        DisposableEffectResult::new(move || space.remove(id))
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
