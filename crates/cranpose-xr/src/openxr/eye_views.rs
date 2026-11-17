//! The two eyes' viewpoints in a frame.
#![expect(
    unsafe_code,
    reason = "xrLocateViews with views on the stack, because the safe wrapper allocates a vector"
)]

use std::{array, ptr};

use openxr as xr;

use super::session::pose_of;
use crate::{FieldOfView, RenderViewpoint};

const VIEW_COUNT: usize = 2;

// The left and the right eye's viewpoint in `local` at `time`. An eye is `None` while the runtime
// does not track the head. The views sit in arrays on the stack, so the frame allocates nothing.
pub(crate) fn locate(
    instance: &xr::Instance,
    session: &xr::Session<xr::OpenGlEs>,
    local: &xr::Space,
    time: xr::Time,
) -> [Option<RenderViewpoint>; VIEW_COUNT] {
    let info = xr::sys::ViewLocateInfo {
        ty: xr::sys::ViewLocateInfo::TYPE,
        next: ptr::null(),
        view_configuration_type: xr::ViewConfigurationType::PRIMARY_STEREO,
        display_time: time,
        space: local.as_raw(),
    };
    let mut state = xr::sys::ViewState {
        ty: xr::sys::ViewState::TYPE,
        next: ptr::null_mut(),
        view_state_flags: xr::ViewStateFlags::EMPTY,
    };
    let mut views: [xr::sys::View; VIEW_COUNT] = array::from_fn(|_| xr::sys::View {
        ty: xr::sys::View::TYPE,
        next: ptr::null_mut(),
        pose: xr::Posef::IDENTITY,
        fov: xr::Fovf {
            angle_left: 0.0,
            angle_right: 0.0,
            angle_up: 0.0,
            angle_down: 0.0,
        },
    });
    let mut count = 0;
    // SAFETY: the session handle is live, `info` and `state` are initialized with their types, and
    // `views` has room for the capacity that the call receives.
    let result = unsafe {
        (instance.fp().locate_views)(
            session.as_raw(),
            &info,
            &mut state,
            VIEW_COUNT as u32,
            &mut count,
            views.as_mut_ptr(),
        )
    };
    let tracked = xr::ViewStateFlags::POSITION_VALID | xr::ViewStateFlags::ORIENTATION_VALID;
    if result.into_raw() < 0 || !state.view_state_flags.contains(tracked) {
        return [None; VIEW_COUNT];
    }
    let located = count as usize;
    array::from_fn(|eye| {
        (eye < located).then(|| RenderViewpoint {
            pose: pose_of(&views[eye].pose),
            field_of_view: FieldOfView {
                angle_left: views[eye].fov.angle_left,
                angle_right: views[eye].fov.angle_right,
                angle_up: views[eye].fov.angle_up,
                angle_down: views[eye].fov.angle_down,
            },
        })
    })
}
