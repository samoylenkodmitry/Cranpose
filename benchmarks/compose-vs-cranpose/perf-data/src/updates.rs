// The frames a screen moves on to, one line each, for a harness that counts
// the app's own content updates beside the presents a tool outside it counts
// (`linux.py` with MangoHud). Only a launch with `PERF_UPDATES` in its
// environment writes them.

use std::sync::OnceLock;

static COUNTED: OnceLock<bool> = OnceLock::new();

// Writes `PERF update FRAME` to stderr when the launch counts updates: the
// screen shows frame `frame`, new content, from now on.
pub fn shown(frame: u32) {
    if *COUNTED.get_or_init(|| std::env::var_os("PERF_UPDATES").is_some()) {
        eprintln!("PERF update {frame}");
    }
}
