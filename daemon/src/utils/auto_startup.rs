use std::process::Command;

/// Set auto startup state (enable or disable) via systemd user service.
pub fn set_auto_startup(enable: bool) -> Result<(), String> {
    let action = if enable { "enable" } else { "disable" };
    let output = Command::new("systemctl")
        .args(["--user", action, "sorakey"])
        .output()
        .map_err(|e| format!("Failed to run systemctl: {}", e))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// Get current auto startup state via systemctl is-enabled.
pub fn get_auto_startup_state() -> bool {
    Command::new("systemctl")
        .args(["--user", "is-enabled", "sorakey"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
