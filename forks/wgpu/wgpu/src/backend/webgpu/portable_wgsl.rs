//! WGSL for browsers whose compiler lacks the `texture_and_sampler_let`
//! language feature, such as Safari 26. Their compiler rejects a `let` that
//! names a texture or a sampler, which Chrome accepts and shaders such as
//! Bevy's use.

use alloc::string::String;

use naga::{Function, Type, TypeInner, UniqueArena, back::wgsl, front, valid};

/// Whether the browser's WGSL compiler accepts a `let` that names a texture
/// or a sampler.
pub(super) fn browser_has_handle_lets() -> bool {
    match super::get_browser_gpu_property() {
        Ok(Some(gpu)) => gpu
            .wgsl_language_features()
            .has("texture_and_sampler_let"),
        _ => true,
    }
}

/// `code` with every `let` of a texture or a sampler removed and the texture
/// or sampler written where the `let`'s name was used. `None` when `code` has
/// no such `let` or naga cannot read it, so the browser gets `code` itself.
pub(super) fn without_handle_lets(code: &str) -> Option<String> {
    let mut module = front::wgsl::parse_str(code).ok()?;
    let info = valid::Validator::new(valid::ValidationFlags::all(), valid::Capabilities::all())
        .validate(&module)
        .ok()?;
    let mut forgot = false;
    for (handle, function) in module.functions.iter_mut() {
        forgot |= forget_handle_names(function, &info[handle], &module.types);
    }
    for (index, entry) in module.entry_points.iter_mut().enumerate() {
        forgot |= forget_handle_names(
            &mut entry.function,
            info.get_entry_point(index),
            &module.types,
        );
    }
    if !forgot {
        return None;
    }
    wgsl::write_string(&module, &info, wgsl::WriterFlags::empty()).ok()
}

// naga's writer gives a named value a `let`. A texture or sampler without a
// name is written as the global or argument it reads.
fn forget_handle_names(
    function: &mut Function,
    info: &valid::FunctionInfo,
    types: &UniqueArena<Type>,
) -> bool {
    let named = function.named_expressions.len();
    function.named_expressions.retain(|&handle, _| {
        !matches!(
            info[handle].ty.inner_with(types),
            TypeInner::Image { .. } | TypeInner::Sampler { .. } | TypeInner::BindingArray { .. }
        )
    });
    function.named_expressions.len() != named
}
