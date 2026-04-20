use super::*;

#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
fn debug_scope_label_env_enabled() -> bool {
    use std::sync::OnceLock;
    static DEBUG_SCOPE_TRACKING: OnceLock<bool> = OnceLock::new();
    *DEBUG_SCOPE_TRACKING.get_or_init(|| std::env::var_os("CRANPOSE_DEBUG_SCOPE_LABELS").is_some())
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

#[cfg(any(not(debug_assertions), target_arch = "wasm32"))]
fn debug_scope_tracking_enabled() -> bool {
    false
}

#[doc(hidden)]
pub fn debug_label_current_scope(name: &'static str) {
    if !debug_scope_tracking_enabled() {
        let _ = name;
        return;
    }
    #[cfg(debug_assertions)]
    with_current_composer(|composer| {
        if let Some(scope) = composer.current_recranpose_scope() {
            DEBUG_SCOPE_LABELS.with(|labels| {
                labels.borrow_mut().insert(scope.id(), name);
            });
        }
    });
}

pub(crate) fn debug_record_scope_invalidation<T: 'static>(
    scope_id: usize,
    state_id: Option<StateId>,
) {
    if !debug_scope_tracking_enabled() {
        let _ = scope_id;
        let _ = state_id;
        return;
    }
    #[cfg(debug_assertions)]
    {
        let source = match state_id {
            Some(id) => format!(
                "slot={} gen={} {}",
                id.slot(),
                id.generation(),
                std::any::type_name::<T>()
            ),
            None => std::any::type_name::<T>().to_string(),
        };
        DEBUG_SCOPE_INVALIDATION_SOURCES.with(|sources| {
            sources
                .borrow_mut()
                .entry(scope_id)
                .or_default()
                .insert(source);
        });
    }
}

#[doc(hidden)]
pub fn debug_scope_label(scope_id: usize) -> Option<&'static str> {
    if !debug_scope_tracking_enabled() {
        let _ = scope_id;
        return None;
    }
    #[cfg(debug_assertions)]
    {
        return DEBUG_SCOPE_LABELS.with(|labels| labels.borrow().get(&scope_id).copied());
    }
    #[allow(unreachable_code)]
    None
}

#[doc(hidden)]
pub fn debug_scope_invalidation_sources(scope_id: usize) -> Vec<String> {
    if !debug_scope_tracking_enabled() {
        let _ = scope_id;
        return Vec::new();
    }
    #[cfg(debug_assertions)]
    {
        return DEBUG_SCOPE_INVALIDATION_SOURCES.with(|sources| {
            let Some(entries) = sources.borrow().get(&scope_id).cloned() else {
                return Vec::new();
            };
            let mut entries: Vec<_> = entries.into_iter().collect();
            entries.sort();
            entries
        });
    }
    #[allow(unreachable_code)]
    Vec::new()
}

#[doc(hidden)]
pub fn debug_live_recompose_scope_count() -> usize {
    LIVE_RECOMPOSE_SCOPE_COUNT.load(Ordering::Relaxed)
}

#[doc(hidden)]
pub fn debug_recompose_scope_registry_stats() -> RecomposeScopeRegistryDebugStats {
    let live = LIVE_RECOMPOSE_SCOPE_COUNT.load(Ordering::Relaxed);
    RecomposeScopeRegistryDebugStats {
        len: live,
        capacity: live,
    }
}
