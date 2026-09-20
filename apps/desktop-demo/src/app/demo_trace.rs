macro_rules! trace {
    ($($arg:tt)*) => {
        if $crate::app::demo_trace::enabled() {
            $crate::app::demo_trace::print(format_args!($($arg)*));
        }
    };
}

pub(crate) use trace;

pub(crate) fn enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("CRANPOSE_DEMO_TRACE").is_some())
}

pub(crate) fn print(args: std::fmt::Arguments<'_>) {
    println!("demo trace: {args}");
}
