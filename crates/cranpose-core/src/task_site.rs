/// Where an effect was written, carried onto the task it spawns.
///
/// A leak report that can only say "one task is still queued" names nothing.
/// Every run prints the same number. No run says which effect it was. The
/// site turns that count into a place to look. It is a pair of plain fields.
/// Carrying it costs nothing until a task is spawned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskSite {
    /// The path of the source file, as `file!()` gives it.
    pub file: &'static str,
    /// The line number in `file`, counted from 1.
    pub line: u32,
}

impl TaskSite {
    /// The site a macro captured with `file!()` and `line!()`.
    pub const fn new(file: &'static str, line: u32) -> Self {
        Self { file, line }
    }
}

impl Default for TaskSite {
    fn default() -> Self {
        Self::new("unknown", 0)
    }
}

impl std::fmt::Display for TaskSite {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}:{}", self.file, self.line)
    }
}

impl From<&'static std::panic::Location<'static>> for TaskSite {
    fn from(location: &'static std::panic::Location<'static>) -> Self {
        Self::new(location.file(), location.line())
    }
}

// What a spawned UI task is called in leak reports. A site is formatted only
// when a report asks. Spawning a task allocates no name.
#[derive(Clone, Copy)]
pub(crate) enum TaskLabel {
    Unnamed,
    Site(TaskSite),
}

impl std::fmt::Display for TaskLabel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unnamed => formatter.write_str("unnamed"),
            Self::Site(site) => site.fmt(formatter),
        }
    }
}
