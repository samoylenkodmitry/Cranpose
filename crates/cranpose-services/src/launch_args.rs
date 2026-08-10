//! Launch arguments — the typed parameters the app was started with.
//!
//! This is the Cranpose equivalent of reading `intent.extras` in a Jetpack
//! Compose activity. A Cranpose app on Android is a `NativeActivity`, so it
//! sees neither the environment of the shell that ran `am start` nor, until
//! now, the launching `Intent`; debug and instrumentation flags read through
//! `std::env::var` silently return nothing on device. [`launch_args`] gives
//! the same values back, typed, on every platform.
//!
//! Where the values come from:
//!
//! * **Android** — the extras of the launching `Intent`, pushed in by the
//!   `cranpose::android` backend (`adb shell am start ... --ez flag true`).
//!   `onNewIntent` replaces the snapshot, exactly as `setIntent` replaces what
//!   `getIntent().getExtras()` returns for a Compose activity.
//! * **Desktop and iOS** — the process command line (the built-in default
//!   below). Command line rather than environment because argv *is* the launch
//!   payload: it is per-launch, it is not inherited by child processes, and it
//!   is what the platform tooling already passes — `xcrun simctl launch`,
//!   `XCUIApplication().launchArguments`, and a plain shell invocation all set
//!   argv, while an exported environment variable leaks into every later
//!   process in that session and cannot be replaced on relaunch.
//! * **Web** — nothing by default; a shell may install the query string.
//!
//! Values keep the type the platform delivered (`--ez`/`--ei`/`--el`/`--ef`/
//! `--es` on Android), and text values are parsed on demand, so an argument
//! written as text on the command line still reads back as a number.
//!
//! ```no_run
//! use cranpose_services::launch_args;
//!
//! let args = launch_args();
//! if args.is_debuggable() && args.boolean("ob_debug").unwrap_or(false) {
//!     let level = args.int("ob_level").unwrap_or(0);
//!     let seed = args.long("ob_seed").unwrap_or(0);
//!     let time_scale = args.float("ob_time_scale").unwrap_or(1.0);
//!     let screen = args.string("ob_screen").unwrap_or("");
//!     let _ = (level, seed, time_scale, screen);
//! }
//! ```

use cranpose_core::{compositionLocalOfWithPolicy, CompositionLocal, CompositionLocalProvider};
use cranpose_macros::composable;
use std::cell::RefCell;
use std::rc::Rc;

/// One launch-argument value, in the type the platform delivered it.
///
/// Android extras arrive already typed. The command-line and query-string
/// backends have no type information, so they deliver [`LaunchArgValue::Text`]
/// and let the typed accessors parse it.
#[derive(Clone, Debug, PartialEq)]
pub enum LaunchArgValue {
    /// `am start --ez name true`, or a bare `--name` on the command line.
    Bool(bool),
    /// `am start --ei name 3`.
    Int(i32),
    /// `am start --el name 90000000000`.
    Long(i64),
    /// `am start --ef name 0.5` (Android `double` extras are narrowed here).
    Float(f32),
    /// `am start --es name lobby`, or `--name=lobby` on the command line.
    Text(String),
}

/// The launch arguments the app was started with.
///
/// An immutable snapshot: reading it never allocates, so composables may query
/// it per frame. The platform replaces the whole snapshot when the launch
/// parameters change (Android `onNewIntent`); it never mutates one in place.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LaunchArgs {
    /// Insertion-ordered; a launch carries a handful of arguments, so a linear
    /// scan beats a map that would have to be allocated and hashed.
    entries: Vec<(Box<str>, LaunchArgValue)>,
    debuggable: bool,
}

/// Shared handle to a [`LaunchArgs`] snapshot.
pub type LaunchArgsRef = Rc<LaunchArgs>;

impl LaunchArgs {
    /// Builds a snapshot from named values.
    ///
    /// Platform backends call this; apps read [`launch_args`] instead. A
    /// repeated name keeps the first value, matching a `Bundle`, where the
    /// later `putExtra` of the same key is what the caller has to avoid.
    pub fn new(
        entries: impl IntoIterator<Item = (String, LaunchArgValue)>,
        debuggable: bool,
    ) -> Self {
        let mut collected: Vec<(Box<str>, LaunchArgValue)> = Vec::new();
        for (name, value) in entries {
            if name.is_empty() || collected.iter().any(|(known, _)| **known == *name) {
                continue;
            }
            collected.push((name.into_boxed_str(), value));
        }
        Self {
            entries: collected,
            debuggable,
        }
    }

    /// Whether the OS considers this build debuggable — Android's
    /// `ApplicationInfo.FLAG_DEBUGGABLE`, `cfg!(debug_assertions)` elsewhere.
    ///
    /// Gate debug and instrumentation options on this. It is the only check
    /// that stays correct in a shipped build: a release APK reports `false`
    /// even when someone passes the extras, so the options cannot be turned on
    /// from the outside.
    pub fn is_debuggable(&self) -> bool {
        self.debuggable
    }

    /// Whether an argument with this name was supplied, whatever its type.
    ///
    /// The reference pattern is a single presence flag (`ob_debug`) that
    /// switches a whole block of options on.
    pub fn contains(&self, name: &str) -> bool {
        self.value(name).is_some()
    }

    /// The names supplied, in the order the platform reported them.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(name, _)| &**name)
    }

    /// The number of arguments supplied.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the app was launched without any arguments.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The raw value, in the type the platform delivered.
    pub fn value(&self, name: &str) -> Option<&LaunchArgValue> {
        self.entries
            .iter()
            .find(|(known, _)| &**known == name)
            .map(|(_, value)| value)
    }

    /// Reads a boolean argument.
    ///
    /// Accepts a `Bool` value, or text spelled `true`/`false`, `1`/`0`,
    /// `yes`/`no`, `on`/`off` in any case. A number is *not* coerced: an
    /// `--ei flag 1` that was meant to be `--ez flag true` reads as `None`
    /// rather than silently enabling something.
    pub fn boolean(&self, name: &str) -> Option<bool> {
        match self.value(name)? {
            LaunchArgValue::Bool(value) => Some(*value),
            LaunchArgValue::Text(text) => parse_boolean(text),
            _ => None,
        }
    }

    /// Reads a 32-bit integer argument.
    ///
    /// Accepts an `Int`, a `Long` that fits, or text.
    pub fn int(&self, name: &str) -> Option<i32> {
        match self.value(name)? {
            LaunchArgValue::Int(value) => Some(*value),
            LaunchArgValue::Long(value) => i32::try_from(*value).ok(),
            LaunchArgValue::Text(text) => text.trim().parse().ok(),
            _ => None,
        }
    }

    /// Reads a 64-bit integer argument.
    ///
    /// Accepts a `Long`, an `Int`, or text. Seeds are the usual case, and a
    /// seed written as `--ei` still reads back here.
    pub fn long(&self, name: &str) -> Option<i64> {
        match self.value(name)? {
            LaunchArgValue::Long(value) => Some(*value),
            LaunchArgValue::Int(value) => Some(i64::from(*value)),
            LaunchArgValue::Text(text) => text.trim().parse().ok(),
            _ => None,
        }
    }

    /// Reads a float argument.
    ///
    /// Accepts a `Float`, an integer widened to `f32`, or text.
    pub fn float(&self, name: &str) -> Option<f32> {
        match self.value(name)? {
            LaunchArgValue::Float(value) => Some(*value),
            LaunchArgValue::Int(value) => Some(*value as f32),
            LaunchArgValue::Long(value) => Some(*value as f32),
            LaunchArgValue::Text(text) => text.trim().parse().ok(),
            _ => None,
        }
    }

    /// Reads a text argument.
    ///
    /// Only a value that was delivered as text answers here, matching
    /// `Bundle.getString`, which returns `null` for an `int` extra. Numbers are
    /// not formatted back into strings.
    pub fn string(&self, name: &str) -> Option<&str> {
        match self.value(name)? {
            LaunchArgValue::Text(text) => Some(text),
            _ => None,
        }
    }
}

fn parse_boolean(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

thread_local! {
    /// Snapshot pushed by the platform backend (Android intent extras today).
    static PLATFORM_LAUNCH_ARGS: RefCell<Option<LaunchArgsRef>> = const { RefCell::new(None) };
    /// The command-line default, parsed once: `launch_args()` is called from
    /// composition, which must not walk argv on every frame.
    static DEFAULT_LAUNCH_ARGS: RefCell<Option<LaunchArgsRef>> = const { RefCell::new(None) };
}

/// Installs the launch arguments reported by the platform, replacing any
/// previous snapshot.
///
/// Android calls this at startup with the launching intent's extras, and again
/// from `onNewIntent`. A backend that replaces the snapshot after startup must
/// also force a root render, because a plain shared cell is not reactive.
pub fn set_platform_launch_args(args: LaunchArgsRef) {
    PLATFORM_LAUNCH_ARGS.with(|cell| *cell.borrow_mut() = Some(args));
}

/// Removes any platform-reported launch arguments (tests and teardown).
pub fn clear_platform_launch_args() {
    PLATFORM_LAUNCH_ARGS.with(|cell| *cell.borrow_mut() = None);
}

/// The launch arguments this app was started with.
///
/// The platform snapshot if one was installed, otherwise the command line.
pub fn launch_args() -> LaunchArgsRef {
    if let Some(args) = PLATFORM_LAUNCH_ARGS.with(|cell| cell.borrow().clone()) {
        return args;
    }
    DEFAULT_LAUNCH_ARGS.with(|cell| {
        let mut cached = cell.borrow_mut();
        cached
            .get_or_insert_with(|| Rc::new(default_launch_args()))
            .clone()
    })
}

/// Whether the OS considers this build debuggable — see
/// [`LaunchArgs::is_debuggable`].
pub fn is_debuggable() -> bool {
    launch_args().is_debuggable()
}

fn default_launch_args() -> LaunchArgs {
    #[cfg(not(target_arch = "wasm32"))]
    {
        launch_args_from_command_line(std::env::args().skip(1), cfg!(debug_assertions))
    }
    #[cfg(target_arch = "wasm32")]
    {
        LaunchArgs::new(std::iter::empty(), cfg!(debug_assertions))
    }
}

/// Parses `--name=value` (text) and bare `--name` (true) out of a command line.
///
/// A lone `--` ends parsing, and everything that is not an option is ignored,
/// so an app that also takes positional arguments keeps them to itself. This is
/// deliberately the smallest convention that round-trips the Android extras:
/// `--ez f true` is `--f` or `--f=true`, `--ei n 3` is `--n=3`.
pub fn launch_args_from_command_line(
    tokens: impl IntoIterator<Item = String>,
    debuggable: bool,
) -> LaunchArgs {
    let mut entries = Vec::new();
    for token in tokens {
        if token == "--" {
            break;
        }
        let Some(option) = token.strip_prefix("--") else {
            continue;
        };
        match option.split_once('=') {
            Some((name, value)) => {
                entries.push((name.to_string(), LaunchArgValue::Text(value.to_string())))
            }
            None => entries.push((option.to_string(), LaunchArgValue::Bool(true))),
        }
    }
    LaunchArgs::new(entries, debuggable)
}

/// The composition local carrying the launch arguments.
///
/// Compares by pointer: a snapshot is replaced wholesale, never edited, so
/// identity is the change signal and no deep comparison is needed per read.
pub fn local_launch_args() -> CompositionLocal<LaunchArgsRef> {
    thread_local! {
        static LOCAL_LAUNCH_ARGS: RefCell<Option<CompositionLocal<LaunchArgsRef>>> = const { RefCell::new(None) };
    }

    LOCAL_LAUNCH_ARGS.with(|cell| {
        let mut local = cell.borrow_mut();
        local
            .get_or_insert_with(|| compositionLocalOfWithPolicy(launch_args, Rc::ptr_eq))
            .clone()
    })
}

/// Provides launch arguments to `content`.
///
/// The platform drivers wrap the app root in this so a mid-session replacement
/// (Android `onNewIntent`) is observed; tests use it to stand in for a launch.
#[allow(non_snake_case)]
#[composable]
pub fn ProvideLaunchArgs(args: LaunchArgsRef, content: impl FnOnce()) {
    let local = local_launch_args();
    CompositionLocalProvider(vec![local.provides(args)], move || {
        content();
    });
}

/// Whether the OS considers this build debuggable, read from composition.
#[allow(non_snake_case)]
#[composable]
pub fn isDebuggable() -> bool {
    local_launch_args().current().is_debuggable()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_test_composition;
    use std::cell::RefCell as StdRefCell;

    fn args(entries: &[(&str, LaunchArgValue)]) -> LaunchArgs {
        LaunchArgs::new(
            entries
                .iter()
                .map(|(name, value)| ((*name).to_string(), value.clone())),
            false,
        )
    }

    fn command_line(tokens: &[&str]) -> LaunchArgs {
        launch_args_from_command_line(tokens.iter().map(|token| (*token).to_string()), false)
    }

    #[test]
    fn typed_extras_read_back_in_the_type_they_arrived_in() {
        let args = args(&[
            ("ob_autoplay", LaunchArgValue::Bool(true)),
            ("ob_level", LaunchArgValue::Int(7)),
            ("ob_seed", LaunchArgValue::Long(9_000_000_000)),
            ("ob_time_scale", LaunchArgValue::Float(0.5)),
            ("ob_screen", LaunchArgValue::Text("lobby".to_string())),
        ]);

        assert_eq!(args.boolean("ob_autoplay"), Some(true));
        assert_eq!(args.int("ob_level"), Some(7));
        assert_eq!(args.long("ob_seed"), Some(9_000_000_000));
        assert_eq!(args.float("ob_time_scale"), Some(0.5));
        assert_eq!(args.string("ob_screen"), Some("lobby"));
    }

    #[test]
    fn a_missing_argument_reads_as_none_for_every_type() {
        let args = args(&[]);

        assert_eq!(args.boolean("absent"), None);
        assert_eq!(args.int("absent"), None);
        assert_eq!(args.long("absent"), None);
        assert_eq!(args.float("absent"), None);
        assert_eq!(args.string("absent"), None);
        assert!(!args.contains("absent"));
        assert!(args.is_empty());
    }

    #[test]
    fn text_arguments_parse_into_the_requested_number_type() {
        let args = args(&[
            ("level", LaunchArgValue::Text("7".to_string())),
            ("seed", LaunchArgValue::Text("9000000000".to_string())),
            ("scale", LaunchArgValue::Text("0.25".to_string())),
            ("flag", LaunchArgValue::Text("ON".to_string())),
        ]);

        assert_eq!(args.int("level"), Some(7));
        assert_eq!(args.long("seed"), Some(9_000_000_000));
        assert_eq!(args.float("scale"), Some(0.25));
        assert_eq!(args.boolean("flag"), Some(true));
        assert_eq!(args.int("seed"), None, "a long that does not fit an i32");
        assert_eq!(args.boolean("level"), None, "numbers are not truthy");
    }

    #[test]
    fn integer_arguments_widen_but_do_not_become_text() {
        let args = args(&[("level", LaunchArgValue::Int(7))]);

        assert_eq!(args.long("level"), Some(7));
        assert_eq!(args.float("level"), Some(7.0));
        assert_eq!(args.string("level"), None);
    }

    #[test]
    fn the_command_line_maps_flags_and_assignments_to_arguments() {
        let args = command_line(&[
            "--ob_debug",
            "--ob_level=7",
            "positional",
            "--ob_screen=lobby",
        ]);

        assert_eq!(args.boolean("ob_debug"), Some(true));
        assert_eq!(args.int("ob_level"), Some(7));
        assert_eq!(args.string("ob_screen"), Some("lobby"));
        assert_eq!(
            args.len(),
            3,
            "positional arguments are not launch arguments"
        );
    }

    #[test]
    fn the_command_line_stops_at_a_bare_double_dash() {
        let args = command_line(&["--before", "--", "--after"]);

        assert!(args.contains("before"));
        assert!(!args.contains("after"));
    }

    #[test]
    fn the_first_value_wins_when_a_name_repeats() {
        let args = args(&[
            ("level", LaunchArgValue::Int(1)),
            ("level", LaunchArgValue::Int(2)),
        ]);

        assert_eq!(args.int("level"), Some(1));
        assert_eq!(args.len(), 1);
    }

    #[test]
    fn the_installed_platform_snapshot_takes_precedence() {
        clear_platform_launch_args();
        set_platform_launch_args(Rc::new(args(&[(
            "ob_autoplay",
            LaunchArgValue::Bool(true),
        )])));

        assert_eq!(launch_args().boolean("ob_autoplay"), Some(true));

        clear_platform_launch_args();
        assert_eq!(launch_args().boolean("ob_autoplay"), None);
    }

    #[test]
    fn debuggable_is_reported_by_the_snapshot() {
        clear_platform_launch_args();
        set_platform_launch_args(Rc::new(LaunchArgs::new(std::iter::empty(), true)));
        assert!(is_debuggable());

        set_platform_launch_args(Rc::new(LaunchArgs::new(std::iter::empty(), false)));
        assert!(!is_debuggable());
        clear_platform_launch_args();
    }

    #[test]
    fn provide_launch_args_reaches_composition() {
        let captured = Rc::new(StdRefCell::new(None));

        {
            let captured = Rc::clone(&captured);
            run_test_composition(move || {
                let captured = Rc::clone(&captured);
                let provided = Rc::new(LaunchArgs::new(
                    [("ob_level".to_string(), LaunchArgValue::Int(3))],
                    true,
                ));
                ProvideLaunchArgs(provided, move || {
                    *captured.borrow_mut() = Some((
                        local_launch_args().current().int("ob_level"),
                        isDebuggable(),
                    ));
                });
            });
        }

        assert_eq!(*captured.borrow(), Some((Some(3), true)));
    }
}
