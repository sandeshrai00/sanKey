/// Path and file system utility functions
use crate::state::paths;
use std::fs;

/// Get absolute path for soundpacks directory (built-in soundpacks)
pub fn get_soundpacks_dir_absolute() -> String {
    paths::soundpacks::get_builtin_soundpacks_dir()
        .to_string_lossy()
        .to_string()
}

/// Create directory recursively if it doesn't exist
pub fn ensure_directory_exists(path: impl AsRef<std::path::Path>) -> Result<(), String> {
    let path_ref = path.as_ref();
    fs::create_dir_all(path_ref)
        .map_err(|e| format!("Failed to create directory '{}': {}", path_ref.display(), e))
}

/// Read file contents as string
pub fn read_file_contents(path: &str) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("Failed to read file '{}': {}", path, e))
}
