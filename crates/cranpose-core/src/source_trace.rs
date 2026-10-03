//! Optional composition origins for development tools.

#[cfg(feature = "inspection")]
use std::cell::RefCell;

/// A composable definition active when a layout node was emitted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceLocation {
    /// Composable function name.
    pub name: &'static str,
    /// Source file reported by the compiler.
    pub file: &'static str,
    /// One-based declaration line.
    pub line: u32,
    /// Package directory reported by the compiler.
    pub manifest_dir: &'static str,
    #[cfg(all(feature = "inspection", debug_assertions))]
    recompositions: Option<RecompositionCounter>,
}

impl SourceLocation {
    /// Body executions after the initial composition for this composable instance.
    /// Returns `None` unless a debug preview enabled recomposition tracking.
    pub fn recompositions(&self) -> Option<u64> {
        #[cfg(all(feature = "inspection", debug_assertions))]
        return self
            .recompositions
            .as_ref()
            .map(|counter| counter.0.get().saturating_sub(1));
        #[cfg(not(all(feature = "inspection", debug_assertions)))]
        None
    }
}

#[cfg(all(feature = "inspection", debug_assertions))]
#[derive(Clone, Debug, Default)]
pub(crate) struct RecompositionCounter(std::rc::Rc<std::cell::Cell<u64>>);

#[cfg(all(feature = "inspection", debug_assertions))]
impl PartialEq for RecompositionCounter {
    fn eq(&self, other: &Self) -> bool {
        std::rc::Rc::ptr_eq(&self.0, &other.0)
    }
}

#[cfg(all(feature = "inspection", debug_assertions))]
impl Eq for RecompositionCounter {}

#[cfg(all(feature = "inspection", debug_assertions))]
thread_local! {
    static TRACK_RECOMPOSITIONS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Enables counters for composable instances on this preview's composition thread.
/// Tracking defaults to disabled and is compiled out of release builds and builds
/// without `inspection`. Skipped bodies and source call-site markers are not counted.
pub fn set_recomposition_tracking(enabled: bool) {
    #[cfg(all(feature = "inspection", debug_assertions))]
    TRACK_RECOMPOSITIONS.with(|tracking| tracking.set(enabled));
    #[cfg(not(all(feature = "inspection", debug_assertions)))]
    let _ = enabled;
}

#[cfg(all(feature = "inspection", debug_assertions))]
fn record_composition(name: &str) -> Option<RecompositionCounter> {
    if !TRACK_RECOMPOSITIONS.with(std::cell::Cell::get) || name.starts_with("__cranpose_call:") {
        return None;
    }
    crate::with_current_composer_opt(|composer| {
        let scope = composer.current_recompose_scope()?;
        let counter = scope
            .inner
            .recompositions
            .get_or_init(RecompositionCounter::default);
        counter.0.set(counter.0.get().saturating_add(1));
        Some(counter.clone())
    })
    .flatten()
}

#[cfg(feature = "inspection")]
thread_local! {
    static STACK: RefCell<Vec<SourceLocation>> = const { RefCell::new(Vec::new()) };
}

/// Restores the composition origin stack when the function returns or unwinds.
#[doc(hidden)]
pub struct SourceScope {
    #[cfg(feature = "inspection")]
    depth: usize,
    marker: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl Drop for SourceScope {
    fn drop(&mut self) {
        #[cfg(feature = "inspection")]
        STACK.with(|stack| stack.borrow_mut().truncate(self.depth));
    }
}

/// Used by the composable macro; compiles to an empty guard without inspection.
#[doc(hidden)]
#[inline]
pub fn __source_scope(
    name: &'static str,
    file: &'static str,
    line: u32,
    manifest_dir: &'static str,
) -> SourceScope {
    #[cfg(feature = "inspection")]
    let depth = STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        let depth = stack.len();
        stack.push(SourceLocation {
            name,
            file,
            line,
            manifest_dir,
            #[cfg(debug_assertions)]
            recompositions: record_composition(name),
        });
        depth
    });
    #[cfg(not(feature = "inspection"))]
    let _ = (name, file, line, manifest_dir);
    SourceScope {
        #[cfg(feature = "inspection")]
        depth,
        marker: std::marker::PhantomData,
    }
}

/// Captures the active composable definitions, outermost first.
#[cfg(feature = "inspection")]
pub fn current_source_trace() -> std::rc::Rc<[SourceLocation]> {
    STACK.with(|stack| std::rc::Rc::from(stack.borrow().as_slice()))
}

#[cfg(feature = "inspection")]
pub(crate) struct SourceContext(Vec<SourceLocation>);

#[cfg(feature = "inspection")]
impl Drop for SourceContext {
    fn drop(&mut self) {
        STACK.with(|stack| *stack.borrow_mut() = std::mem::take(&mut self.0));
    }
}

#[cfg(feature = "inspection")]
pub(crate) fn restore_source_trace(trace: &[SourceLocation]) -> SourceContext {
    SourceContext(STACK.with(|stack| stack.replace(trace.to_vec())))
}

#[cfg(all(test, feature = "inspection"))]
#[path = "tests/source_trace_tests.rs"]
mod tests;
