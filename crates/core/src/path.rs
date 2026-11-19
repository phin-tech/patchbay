//! The `PATH` servers run with. An app opened from Finder gets launchd's bare `PATH`, without Homebrew, mise or
//! `~/.local/bin`, so `npx` and `uvx` wouldn't be found. The person's login shell knows the real one.

use std::process::Command;
use std::sync::OnceLock;

/// The login shell's `PATH`, asked once, with the usual places added in case the shell can't be asked.
pub fn login_path() -> &'static str {
    static PATH: OnceLock<String> = OnceLock::new();
    PATH.get_or_init(|| {
        let mut dirs: Vec<String> = shell_path()
            .or_else(|| std::env::var("PATH").ok())
            .unwrap_or_default()
            .split(':')
            .filter(|dir| !dir.is_empty())
            .map(str::to_string)
            .collect();
        let home = std::env::var("HOME").unwrap_or_default();
        for dir in [
            "/opt/homebrew/bin".to_string(),
            "/usr/local/bin".to_string(),
            format!("{home}/.local/bin"),
            format!("{home}/.cargo/bin"),
            "/usr/bin".to_string(),
            "/bin".to_string(),
        ] {
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
        dirs.join(":")
    })
}

/// `PATH` as the login shell sets it, read from `env` so any shell (fish too) answers the same way.
fn shell_path() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let output = Command::new(shell)
        .args(["-l", "-c", "env"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("PATH=").map(str::to_string))
}
