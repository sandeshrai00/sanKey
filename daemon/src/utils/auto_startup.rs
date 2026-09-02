use std::process::Command;

/// Get current auto startup state via systemctl is-enabled.
pub fn get_auto_startup_state() -> bool {
    Command::new("systemctl")
        .args(["--user", "is-enabled", "sorakey"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
