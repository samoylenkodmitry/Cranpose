//! Cached reads of the `CRANPOSE_*` diagnostic environment variables.
//!
//! Every diagnostic switch in the framework is read from the environment, and
//! several of them sit on paths that run once per frame, once per state write,
//! or once per laid-out node. That is a problem, because reading an
//! environment variable is not a cheap lookup: `getenv` takes the environ
//! lock and walks the whole variable array, comparing each entry's name
//! prefix, until it finds a match or runs off the end. A switch that is
//! *turned off* is the worst case — it never matches, so every read pays for
//! a full scan of the process environment just to learn there is nothing to
//! print. On a device that showed up as a measurable share of an idle frame,
//! spent entirely inside `strncmp`.
//!
//! None of these switches can change after startup. Android's are seeded from
//! `debug.cranpose.*` system properties before the app shell exists, and on
//! every other platform they come from the process environment, which the
//! framework never mutates. So each one is read once and cached.
//!
//! Use `env_flag!` for presence switches and `env_threshold_ms!` for the
//! millisecond thresholds. Both expand to a `OnceLock` owned by the call site,
//! which costs an acquire load once warm.

/// Reads a presence-style environment switch once and caches the answer.
///
/// Expands to a `bool` that is `true` when the variable is set to anything at
/// all, including the empty string. The backing `OnceLock` belongs to the
/// expansion, so two call sites naming the same variable get their own cache
/// rather than sharing one; that keeps the macro usable from any crate without
/// a registry, at the cost of one pointer-sized static each.
///
/// ```ignore
/// if env_flag!("CRANPOSE_SCENE_UPDATE_DIAG") {
///     eprintln!("[scene-update-diag] dirty={dirty_nodes:?}");
/// }
/// ```
#[macro_export]
macro_rules! env_flag {
    ($name:expr) => {{
        static CRANPOSE_ENV_FLAG: ::std::sync::OnceLock<bool> = ::std::sync::OnceLock::new();
        *CRANPOSE_ENV_FLAG.get_or_init(|| ::std::env::var_os($name).is_some())
    }};
}

/// Reads a millisecond-threshold environment variable once and caches it.
///
/// Expands to an `Option<f64>`: `None` when the variable is unset or does not
/// parse, `Some(threshold)` otherwise. A threshold of `0` is preserved rather
/// than treated as absent, so `0` means "report every frame" — the setting
/// used to capture a full trace.
///
/// ```ignore
/// if let Some(threshold) = env_threshold_ms!("CRANPOSE_FRAME_STAGE_TELEMETRY_MS") {
///     if elapsed_ms >= threshold {
///         log::info!("[frame-stage] {elapsed_ms:.2}ms");
///     }
/// }
/// ```
#[macro_export]
macro_rules! env_threshold_ms {
    ($name:expr) => {{
        static CRANPOSE_ENV_THRESHOLD: ::std::sync::OnceLock<Option<f64>> =
            ::std::sync::OnceLock::new();
        *CRANPOSE_ENV_THRESHOLD.get_or_init(|| {
            ::std::env::var($name)
                .ok()
                .and_then(|value| value.trim().parse::<f64>().ok())
                .filter(|threshold| *threshold >= 0.0)
        })
    }};
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_unset_switch_reads_false_and_stays_false() {
        assert!(!env_flag!("CRANPOSE_ENV_FLAG_THAT_NOBODY_SETS"));
        assert!(!env_flag!("CRANPOSE_ENV_FLAG_THAT_NOBODY_SETS"));
    }

    #[test]
    fn an_unset_threshold_reads_none() {
        assert_eq!(env_threshold_ms!("CRANPOSE_ENV_MS_THAT_NOBODY_SETS"), None);
    }

    #[test]
    fn each_call_site_caches_the_value_it_read_first() {
        let first = env_flag!("CRANPOSE_ENV_FLAG_CACHE_PROBE");
        let second = env_flag!("CRANPOSE_ENV_FLAG_CACHE_PROBE");
        assert_eq!(first, second);
    }
}
