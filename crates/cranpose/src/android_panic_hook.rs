use std::panic::PanicHookInfo;

pub(crate) fn chained_panic_hook(
    own_hook: impl Fn(&PanicHookInfo<'_>) + Sync + Send + 'static,
    previous_hook: Box<dyn Fn(&PanicHookInfo<'_>) + Sync + Send>,
) -> Box<dyn Fn(&PanicHookInfo<'_>) + Sync + Send> {
    Box::new(move |info| {
        own_hook(info);
        previous_hook(info);
    })
}

#[cfg(test)]
#[path = "tests/android_panic_hook_tests.rs"]
mod tests;
