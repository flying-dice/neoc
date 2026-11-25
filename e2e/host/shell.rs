//! The Terminal app's shell for the end-to-end host on Windows (e2e/host/start.ts compiles this with `rustc`, on
//! Windows only, and makes it the host's `ComSpec`).
//!
//! The host starts the default shell with no arguments, and on Windows the default shell is a program
//! (`CreateProcessW` takes no batch file as one), so this is the program: it starts Windows PowerShell, the one
//! every Windows has, with no banner, no profile and no line editor of its own (PSReadLine redraws and colours the
//! line as it is typed), and with the prompt `$ `. So what the Terminal's window shows is the same on every
//! machine. It gives PowerShell the real `ComSpec` back, waits for it, and ends with its exit code.

use std::env;
use std::path::PathBuf;
use std::process::{Command, exit};

extern "system" {
    fn SetConsoleCtrlHandler(handler: Option<extern "system" fn(u32) -> i32>, add: i32) -> i32;
}

/// Ctrl+C and Ctrl+Break are PowerShell's to answer: this process stays until PowerShell ends.
extern "system" fn stay(event: u32) -> i32 {
    i32::from(event <= 1)
}

fn main() {
    unsafe { SetConsoleCtrlHandler(Some(stay), 1) };
    let system = PathBuf::from(env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into())).join("System32");
    let status = Command::new(system.join(r"WindowsPowerShell\v1.0\powershell.exe"))
        .args(["-NoLogo", "-NoProfile", "-NoExit", "-Command"])
        .arg("Remove-Module PSReadLine -ErrorAction SilentlyContinue; function prompt { '$ ' }")
        .env("ComSpec", system.join("cmd.exe"))
        .status();
    match status {
        Ok(status) => exit(status.code().unwrap_or(1)),
        Err(error) => {
            eprintln!("PowerShell could not be started: {error}");
            exit(1)
        }
    }
}
