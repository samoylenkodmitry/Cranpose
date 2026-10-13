use std::cell::{Cell, RefCell};

use crate::{frame_graph::fence_profile::Profile, scene::SceneBuffers};

pub(crate) struct WgpuThreadState {
    pub(crate) scene_buffer_pool: RefCell<Vec<SceneBuffers>>,
    pub(crate) off_frame_builds: Cell<bool>,
    pub(crate) thread_builds: Cell<u64>,
    pub(crate) upload_write_calls: Cell<u32>,
    pub(crate) render_pass_pixels: Cell<u64>,
    pub(crate) render_pass_labels: RefCell<Vec<(String, u32)>>,
    pub(crate) fence_profile: RefCell<Profile>,
}

impl WgpuThreadState {
    const fn new() -> Self {
        Self {
            scene_buffer_pool: RefCell::new(Vec::new()),
            off_frame_builds: Cell::new(false),
            thread_builds: Cell::new(0),
            upload_write_calls: Cell::new(0),
            render_pass_pixels: Cell::new(0),
            render_pass_labels: RefCell::new(Vec::new()),
            fence_profile: RefCell::new(Profile::new()),
        }
    }
}

thread_local! {
    static STATE: WgpuThreadState = const { WgpuThreadState::new() };
}

pub(crate) fn with<R>(f: impl FnOnce(&WgpuThreadState) -> R) -> R {
    cranpose_core::with_or_fresh(&STATE, WgpuThreadState::new, f)
}

pub(crate) fn try_with<R>(f: impl FnOnce(&WgpuThreadState) -> R) -> Option<R> {
    STATE.try_with(f).ok()
}
