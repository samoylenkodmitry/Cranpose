//! The room around the panels. A runtime shows it in one of two ways:
//!
//! - `XR_FB_passthrough` (the Meta Quest): a layer of camera pictures under all the other
//!   layers, made and started once. The layer joins a frame while its opacity is above 0.
//! - The blend mode `ALPHA_BLEND` (Android XR, Pico, Magic Leap): the runtime mixes the layers'
//!   alpha with the room, so the opacity chooses between that mode and `OPAQUE`.
//!
//! A runtime with neither shows the app's own background, and the app cannot change it.
#![expect(
    unsafe_code,
    reason = "XR_FB_passthrough's functions and its composition layer struct"
)]

use std::ptr;

use openxr::{self as xr, sys, sys::Handle};

// How a runtime shows the room.
enum Backend {
    Layer(PassthroughLayer),
    Blend,
    // The runtime's first blend mode, with no way to change it
    Fixed,
}

pub(crate) struct Passthrough {
    backend: Backend,
    // The blend mode of every frame that `Blend` does not choose
    fixed: xr::EnvironmentBlendMode,
    // How much of the room shows, from 0 to 1
    opacity: f32,
}

impl Passthrough {
    // Chooses how the session shows the room, from the extension the instance enabled and the
    // blend modes the runtime lists.
    pub(crate) fn new(
        session: &xr::Session<xr::OpenGlEs>,
        modes: &[xr::EnvironmentBlendMode],
    ) -> Self {
        let fixed = modes
            .first()
            .copied()
            .unwrap_or(xr::EnvironmentBlendMode::OPAQUE);
        let layer = PassthroughLayer::new(session).unwrap_or_else(|error| {
            log::warn!("cranpose xr: no passthrough layer: {error}");
            None
        });
        let blends = modes.contains(&xr::EnvironmentBlendMode::OPAQUE)
            && modes.contains(&xr::EnvironmentBlendMode::ALPHA_BLEND);
        let backend = match layer {
            Some(layer) => Backend::Layer(layer),
            None if blends => Backend::Blend,
            None => Backend::Fixed,
        };
        let shown =
            matches!(backend, Backend::Fixed) && fixed == xr::EnvironmentBlendMode::ALPHA_BLEND;
        log::info!(
            "cranpose xr: passthrough through {}, blend modes {modes:?}",
            match backend {
                Backend::Layer(_) => "XR_FB_passthrough",
                Backend::Blend => "the blend mode ALPHA_BLEND",
                Backend::Fixed => "nothing: the app cannot change the room",
            }
        );
        Self {
            backend,
            fixed,
            opacity: if shown { 1.0 } else { 0.0 },
        }
    }

    // Whether the app can change how much of the room shows.
    pub(crate) fn is_controllable(&self) -> bool {
        !matches!(self.backend, Backend::Fixed)
    }

    // How much of the room shows, from 0 to 1.
    pub(crate) fn opacity(&self) -> f32 {
        self.opacity
    }

    // Shows `opacity` of the room, from 0 to 1. A runtime that mixes alpha shows all of it above 0.
    pub(crate) fn set_opacity(&mut self, opacity: f32) {
        let opacity = if opacity > 0.0 { opacity.min(1.0) } else { 0.0 };
        let shown = match &self.backend {
            Backend::Layer(_) => opacity,
            Backend::Blend => f32::from(opacity > 0.0),
            Backend::Fixed => return,
        };
        if shown == self.opacity {
            return;
        }
        if let Backend::Layer(layer) = &self.backend
            && opacity > 0.0
            && let Err(error) = layer.set_opacity(opacity)
        {
            log::warn!("cranpose xr: passthrough opacity {opacity}: {error}");
            return;
        }
        log::info!("cranpose xr: passthrough opacity {shown}");
        self.opacity = shown;
    }

    // The blend mode of the next frame.
    pub(crate) fn blend_mode(&self) -> xr::EnvironmentBlendMode {
        match self.backend {
            Backend::Blend if self.opacity > 0.0 => xr::EnvironmentBlendMode::ALPHA_BLEND,
            Backend::Blend => xr::EnvironmentBlendMode::OPAQUE,
            _ => self.fixed,
        }
    }

    // The layer to put under every other layer of the next frame, if the room shows through one.
    pub(crate) fn layer(&self) -> Option<&xr::CompositionLayerBase<'_, xr::OpenGlEs>> {
        match &self.backend {
            Backend::Layer(layer) if self.opacity > 0.0 => Some(layer.composition()),
            _ => None,
        }
    }
}

// The camera pictures as a layer. Its handles end with it.
struct PassthroughLayer {
    functions: xr::raw::PassthroughFB,
    passthrough: sys::PassthroughFB,
    handle: sys::PassthroughLayerFB,
    composition: sys::CompositionLayerPassthroughFB,
}

impl PassthroughLayer {
    // Makes and starts the passthrough and its layer, or returns `None` where the runtime lacks
    // the extension.
    fn new(session: &xr::Session<xr::OpenGlEs>) -> Result<Option<Self>, String> {
        let Some(functions) = session.instance().exts().fb_passthrough else {
            return Ok(None);
        };
        let mut passthrough = sys::PassthroughFB::NULL;
        let info = sys::PassthroughCreateInfoFB {
            ty: sys::PassthroughCreateInfoFB::TYPE,
            next: ptr::null(),
            flags: sys::PassthroughFlagsFB::IS_RUNNING_AT_CREATION,
        };
        // SAFETY: the session is live, and the function pointers come from its instance, which
        // enabled the extension.
        check(unsafe {
            (functions.create_passthrough)(session.as_raw(), &info, &mut passthrough)
        })?;
        let mut handle = sys::PassthroughLayerFB::NULL;
        let info = sys::PassthroughLayerCreateInfoFB {
            ty: sys::PassthroughLayerCreateInfoFB::TYPE,
            next: ptr::null(),
            passthrough,
            flags: sys::PassthroughFlagsFB::IS_RUNNING_AT_CREATION,
            purpose: sys::PassthroughLayerPurposeFB::RECONSTRUCTION,
        };
        // SAFETY: as above, and `passthrough` is the handle made above.
        let made = check(unsafe {
            (functions.create_passthrough_layer)(session.as_raw(), &info, &mut handle)
        });
        if let Err(error) = made {
            // SAFETY: the handle is the one made above and no layer uses it.
            unsafe { (functions.destroy_passthrough)(passthrough) };
            return Err(error);
        }
        Ok(Some(Self {
            functions,
            passthrough,
            handle,
            composition: sys::CompositionLayerPassthroughFB {
                ty: sys::CompositionLayerPassthroughFB::TYPE,
                next: ptr::null(),
                flags: xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA,
                space: sys::Space::NULL,
                layer_handle: handle,
            },
        }))
    }

    // Sets how opaque the camera pictures are over what lies under them.
    fn set_opacity(&self, opacity: f32) -> Result<(), String> {
        let style = sys::PassthroughStyleFB {
            ty: sys::PassthroughStyleFB::TYPE,
            next: ptr::null(),
            texture_opacity_factor: opacity,
            edge_color: sys::Color4f {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            },
        };
        // SAFETY: the layer handle is live until this struct drops.
        check(unsafe { (self.functions.passthrough_layer_set_style)(self.handle, &style) })
    }

    fn composition(&self) -> &xr::CompositionLayerBase<'_, xr::OpenGlEs> {
        // SAFETY: the base layer is a transparent wrapper of the header that every composition
        // layer struct starts with, as the passthrough layer's does.
        unsafe { &*ptr::from_ref(&self.composition).cast() }
    }
}

impl Drop for PassthroughLayer {
    fn drop(&mut self) {
        // SAFETY: the handles are live, and no frame holds the layer once its owner drops.
        unsafe {
            (self.functions.destroy_passthrough_layer)(self.handle);
            (self.functions.destroy_passthrough)(self.passthrough);
        }
    }
}

fn check(result: sys::Result) -> Result<(), String> {
    if result.into_raw() >= 0 {
        Ok(())
    } else {
        Err(format!("{result:?}"))
    }
}
