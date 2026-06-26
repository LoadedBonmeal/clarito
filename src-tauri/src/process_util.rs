//! Child-process spawning helper.
//!
//! On Windows a GUI app that spawns a *console* subsystem child (the bundled
//! Java declaration validator, the `hostname`/`whoami` license-fingerprint
//! helpers, `cmd /c start …`) makes a black console window flash for a few
//! milliseconds unless the `CREATE_NO_WINDOW` creation flag is set. macOS and
//! Linux have no such flash. `hidden_command` centralises the fix: it returns a
//! `std::process::Command` carrying that flag on Windows and is a plain
//! `Command::new` everywhere else, so every spawn site can use it uniformly.

use std::ffi::OsStr;
use std::process::Command;

/// `Command::new(program)` that never pops a console window on Windows.
///
/// The flag only suppresses the *console* of the spawned process; stdout/stderr
/// still pipe to the parent as usual, and `cmd /c start <url>` still launches the
/// browser/file association — only its stray console is hidden.
pub fn hidden_command<S: AsRef<OsStr>>(program: S) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW (0x0800_0000): the child runs with no console window.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}
