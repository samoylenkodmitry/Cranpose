use std::sync::{
    Mutex, PoisonError,
    atomic::{AtomicUsize, Ordering},
};

use crate::RuntimeShader;

/// The framework's own runtime shader sources. A renderer rebuilds the
/// pipelines its last launch drew the first screen with from these, matched
/// by [`crate::runtime_shader_source_hash`], before the app builds them.
pub static BUILTIN_RUNTIME_SHADER_SOURCES: [&str; 9] = [
    crate::LIQUID_GLASS_WGSL,
    crate::GRADIENT_BLUR_WGSL,
    crate::GRADIENT_CUT_MASK_WGSL,
    crate::ROUNDED_ALPHA_MASK_WGSL,
    crate::GRADIENT_FADE_DST_OUT_WGSL,
    crate::LIQUID_VIBRANCY_WGSL,
    crate::LIQUID_TAB_LIGHTING_WGSL,
    crate::LIQUID_CONTROL_CONTENT_WGSL,
    crate::LIQUID_FLOATING_LIGHTING_WGSL,
];

static REQUESTED: Mutex<Vec<RuntimeShader>> = Mutex::new(Vec::new());
static REQUESTED_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Asks every renderer to compile the pipelines of `shaders` on its
/// background compiler, before a frame needs them.
///
/// Call it where an app first builds what draws them, such as a widget's
/// first composition, so the pipelines are ready before that widget's
/// effect first draws, and an app compiles nothing for effects it never
/// builds. A shader requested twice is kept once. A renderer created later,
/// as after an Android surface loss, compiles every request made before it.
pub fn request_shader_warm_ups(shaders: impl IntoIterator<Item = RuntimeShader>) {
    let mut requested = REQUESTED.lock().unwrap_or_else(PoisonError::into_inner);
    for shader in shaders {
        if !requested.contains(&shader) {
            requested.push(shader);
        }
    }
    REQUESTED_COUNT.store(requested.len(), Ordering::Release);
}

/// The shaders requested after the first `seen`, in the order they were
/// requested: a renderer passes how many it has already queued.
pub fn shader_warm_ups_after(seen: usize) -> Vec<RuntimeShader> {
    if REQUESTED_COUNT.load(Ordering::Acquire) <= seen {
        return Vec::new();
    }
    let requested = REQUESTED.lock().unwrap_or_else(PoisonError::into_inner);
    requested
        .get(seen..)
        .map_or_else(Vec::new, <[RuntimeShader]>::to_vec)
}

#[cfg(test)]
#[path = "tests/shader_warm_up_tests.rs"]
mod tests;
