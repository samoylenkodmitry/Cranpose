use super::*;

#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
fn debug_scope_label_env_enabled() -> bool {
    crate::env_flag!("CRANPOSE_DEBUG_SCOPE_LABELS")
}

#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
fn debug_scope_tracking_enabled() -> bool {
    #[cfg(test)]
    if let Some(enabled) = DEBUG_SCOPE_TRACKING_OVERRIDE.with(Cell::get) {
        return enabled;
    }
    debug_scope_label_env_enabled()
        || log::log_enabled!(target: "cranpose::compose::recompose", log::Level::Trace)
        || log::log_enabled!(target: "cranpose::compose::emit", log::Level::Trace)
        || log::log_enabled!(target: "cranpose::compose::parent", log::Level::Trace)
}

#[cfg(all(debug_assertions, target_arch = "wasm32"))]
fn debug_scope_tracking_enabled() -> bool {
    false
}

#[cfg(debug_assertions)]
#[doc(hidden)]
pub fn debug_label_current_scope(name: &'static str) {
    if !debug_scope_tracking_enabled() {
        return;
    }
    with_current_composer(|composer| {
        if let Some(scope) = composer.current_recompose_scope() {
            DEBUG_SCOPE_LABELS.with(|labels| {
                labels.borrow_mut().insert(scope.id(), name);
            });
        }
    });
}

#[cfg(debug_assertions)]
pub(crate) fn debug_record_scope_invalidation(
    scope_id: usize,
    state_id: Option<StateId>,
    value_type: &'static str,
) {
    if !debug_scope_tracking_enabled() {
        return;
    }
    let source = match state_id {
        Some(id) => format!("slot={} gen={} {value_type}", id.slot(), id.generation()),
        None => value_type.to_string(),
    };
    DEBUG_SCOPE_INVALIDATION_SOURCES.with(|sources| {
        sources
            .borrow_mut()
            .entry(scope_id)
            .or_default()
            .insert(source);
    });
}

#[cfg(debug_assertions)]
#[doc(hidden)]
pub fn debug_scope_label(scope_id: usize) -> Option<&'static str> {
    if !debug_scope_tracking_enabled() {
        return None;
    }
    DEBUG_SCOPE_LABELS.with(|labels| labels.borrow().get(&scope_id).copied())
}

#[cfg(debug_assertions)]
#[doc(hidden)]
pub fn debug_scope_invalidation_sources(scope_id: usize) -> Vec<String> {
    if !debug_scope_tracking_enabled() {
        return Vec::new();
    }
    DEBUG_SCOPE_INVALIDATION_SOURCES.with(|sources| {
        let Some(entries) = sources.borrow().get(&scope_id).cloned() else {
            return Vec::new();
        };
        let mut entries: Vec<_> = entries.into_iter().collect();
        entries.sort();
        entries
    })
}

#[cfg(not(debug_assertions))]
#[doc(hidden)]
pub fn debug_label_current_scope(_name: &'static str) {}

#[cfg(not(debug_assertions))]
pub(crate) fn debug_record_scope_invalidation(
    _scope_id: usize,
    _state_id: Option<StateId>,
    _value_type: &'static str,
) {
}

#[cfg(not(debug_assertions))]
#[doc(hidden)]
pub fn debug_scope_label(_scope_id: usize) -> Option<&'static str> {
    None
}

#[cfg(not(debug_assertions))]
#[doc(hidden)]
pub fn debug_scope_invalidation_sources(_scope_id: usize) -> Vec<String> {
    Vec::new()
}

#[doc(hidden)]
pub fn debug_live_recompose_scope_count() -> usize {
    crate::runtime::live_recompose_scope_count()
}

#[doc(hidden)]
pub fn debug_recompose_scope_registry_stats() -> RecomposeScopeRegistryDebugStats {
    let live = crate::runtime::live_recompose_scope_count();
    RecomposeScopeRegistryDebugStats {
        len: live,
        capacity: live,
    }
}
