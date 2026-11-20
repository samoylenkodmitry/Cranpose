//! The calls of `XR_FB_render_model` that the `openxr` crate's wrappers make with output structs that
//! the runtime may leave unwritten.
//!
//! The crate lists the render model paths into a buffer of structs whose `type` is unset, which the
//! specification asks the application to set. It reads the model's properties from a struct that
//! it never initializes, so a runtime that answers that the model is unavailable, a success code
//! that writes nothing, leaves it garbage. These calls initialize every struct that they pass.
#![expect(
    unsafe_code,
    reason = "the raw calls of the extension, with structs that this file initializes"
)]

use std::ptr;

use openxr as xr;

use super::graphics::Api;

// What the runtime says of a render model. An unavailable model has a null key and no name.
pub(crate) struct ModelProperties {
    pub(crate) key: xr::RenderModelKeyFB,
    pub(crate) name: String,
    pub(crate) vendor: u32,
    pub(crate) version: u32,
    pub(crate) flags: xr::RenderModelFlagsFB,
}

fn table(instance: &xr::Instance) -> Result<&xr::raw::RenderModelFB, String> {
    instance
        .exts()
        .fb_render_model
        .as_ref()
        .ok_or_else(|| "XR_FB_render_model is not loaded".to_owned())
}

// A success code, which may be a positive one such as "unavailable", is no failure.
fn checked(result: xr::sys::Result) -> Result<(), String> {
    if result.into_raw() < 0 {
        Err(format!("{result:?}"))
    } else {
        Ok(())
    }
}

// The paths of the render models that the runtime has.
pub(crate) fn paths(
    session: &xr::Session<Api>,
    instance: &xr::Instance,
) -> Result<Vec<xr::Path>, String> {
    let enumerate = table(instance)?.enumerate_render_model_paths;
    let blank = xr::sys::RenderModelPathInfoFB {
        ty: xr::sys::RenderModelPathInfoFB::TYPE,
        next: ptr::null_mut(),
        path: xr::Path::NULL,
    };
    let mut count = 0;
    // SAFETY: the session handle is live. A capacity of zero asks only for the count, and the
    // output pointer may be null then.
    checked(unsafe { enumerate(session.as_raw(), 0, &mut count, ptr::null_mut()) })?;
    let mut infos = vec![blank; count as usize];
    // SAFETY: `infos` holds `count` structs of the extension's type, each with its `type` set.
    checked(unsafe { enumerate(session.as_raw(), count, &mut count, infos.as_mut_ptr()) })?;
    infos.truncate(count as usize);
    Ok(infos.into_iter().map(|info| info.path).collect())
}

// The properties of the model at `path` for an app that reads the glTF subsets in `flags`.
pub(crate) fn properties(
    session: &xr::Session<Api>,
    instance: &xr::Instance,
    path: xr::Path,
    flags: xr::RenderModelFlagsFB,
) -> Result<ModelProperties, String> {
    let get = table(instance)?.get_render_model_properties;
    let mut request = xr::sys::RenderModelCapabilitiesRequestFB {
        ty: xr::sys::RenderModelCapabilitiesRequestFB::TYPE,
        next: ptr::null_mut(),
        flags,
    };
    // SAFETY: every field of the struct is an integer, a newtype of one or an array of them, and a
    // pointer, so zeros are valid values. The struct is the one that the call fills in.
    let mut found: xr::sys::RenderModelPropertiesFB = unsafe { std::mem::zeroed() };
    found.ty = xr::sys::RenderModelPropertiesFB::TYPE;
    found.next = ptr::from_mut(&mut request).cast();
    // SAFETY: the session handle is live, and `found` and `request` are initialized structs of the
    // types that the extension names. `request` outlives the call.
    checked(unsafe { get(session.as_raw(), path, &mut found) })?;
    let name: Vec<u8> = found
        .model_name
        .iter()
        .take_while(|&&byte| byte != 0)
        .map(|byte| byte.to_ne_bytes()[0])
        .collect();
    Ok(ModelProperties {
        key: found.model_key,
        name: String::from_utf8_lossy(&name).into_owned(),
        vendor: found.vendor_id,
        version: found.model_version,
        flags: found.flags,
    })
}
