use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Load JSON from file.
pub fn load_json_from_file<T>(file_path: &Path) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    let contents = fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read file '{}': {}", file_path.display(), e))?;

    serde_json::from_str::<T>(&contents)
        .map_err(|e| format!("Failed to parse JSON from '{}': {}", file_path.display(), e))
}

/// Save JSON atomically — write to sibling temp file then rename.
pub fn save_json_to_file_atomically<T>(data: &T, file_path: &Path) -> Result<(), String>
where
    T: Serialize,
{
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory '{}': {}", parent.display(), e))?;
    }

    let contents = serde_json::to_string_pretty(data)
        .map_err(|e| format!("Failed to serialize data: {}", e))?;

    // pid in name avoids clashing with another instance
    let temp_path = {
        let mut name = file_path.as_os_str().to_os_string();
        name.push(format!(".{}.tmp", std::process::id()));
        std::path::PathBuf::from(name)
    };

    fs::write(&temp_path, contents)
        .map_err(|e| format!("Failed to write file '{}': {}", temp_path.display(), e))?;

    fs::rename(&temp_path, file_path).map_err(|e| {
        // clean up temp on failure
        let _ = fs::remove_file(&temp_path);
        format!("Failed to replace file '{}': {}", file_path.display(), e)
    })
}
