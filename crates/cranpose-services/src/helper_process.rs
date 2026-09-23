use std::{ffi::OsStr, process::Command};

/// A [`Command`] for a helper program an application runs behind its window,
/// such as `reg`, `powershell` or `notify-send`.
///
/// On Windows the program starts with no console window of its own. A desktop
/// application built as a GUI program has no console to lend, so any console
/// program it starts otherwise opens a terminal window that flashes and
/// closes. Everywhere else this is `Command::new(program)`.
pub fn windowless_command(program: impl AsRef<OsStr>) -> Command {
    let command = Command::new(program);
    #[cfg(windows)]
    let command = {
        use std::os::windows::process::CommandExt;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut command = command;
        command.creation_flags(CREATE_NO_WINDOW);
        command
    };
    command
}
