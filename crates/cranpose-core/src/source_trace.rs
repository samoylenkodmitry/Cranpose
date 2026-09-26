//! Optional composition origins for development tools.

#[cfg(feature = "inspection")]
use std::cell::RefCell;

/// A composable definition active when a layout node was emitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceLocation {
    /// Composable function name.
    pub name: &'static str,
    /// Source file reported by the compiler.
    pub file: &'static str,
    /// One-based declaration line.
    pub line: u32,
    /// Package directory reported by the compiler.
    pub manifest_dir: &'static str,
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
