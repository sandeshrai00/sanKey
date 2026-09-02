//! Fixed layout under `~/.local/share/sorakey` — data + soundpacks.

use std::path::PathBuf;
use std::sync::OnceLock;

fn data_dir() -> PathBuf {
    match directories::BaseDirs::new() {
        Some(b) => b.data_dir().join("sorakey"),
        None => PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".local/share/sorakey"),
    }
}

/// Writable state directory: `~/.local/share/sorakey/data`.
pub fn get_writable_data_dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| data_dir().join("data"))
}

pub mod data {
    use super::get_writable_data_dir;
    use std::path::PathBuf;

    pub fn config_json() -> PathBuf {
        get_writable_data_dir().join("config.json")
    }

    pub fn soundpack_cache_json() -> PathBuf {
        get_writable_data_dir().join("soundpack_cache.json")
    }
}

pub mod soundpacks {
    use super::data_dir;
    use std::path::{ Path, PathBuf };
    use std::sync::OnceLock;

    pub fn get_builtin_soundpacks_dir() -> PathBuf {
        static DIR: OnceLock<PathBuf> = OnceLock::new();
        DIR.get_or_init(|| data_dir().join("soundpacks")).clone()
    }

    pub fn ensure_soundpack_directories() -> std::io::Result<()> {
        std::fs::create_dir_all(get_builtin_soundpacks_dir().join("keyboard"))?;
        Ok(())
    }

    /// Directory for a soundpack id.
    pub fn soundpack_dir(soundpack_id: &str) -> String {
        let sanitized = soundpack_id.replace('\\', "/");
        let parts: Vec<&str> = sanitized.split('/').filter(|p| !p.is_empty() && *p != ".." && !p.contains('\0')).collect();
        let join = |base: &Path| -> PathBuf {
            parts.iter().fold(base.to_path_buf(), |p, part| p.join(part))
        };
        let base = get_builtin_soundpacks_dir();
        let joined = join(Path::new(&base));
        // stay inside base dir
        if !joined.starts_with(&*base) {
            return base.join("keyboard").join("invalid").to_string_lossy().to_string();
        }
        joined.to_string_lossy().to_string()
    }

    pub fn config_json(soundpack_id: &str) -> String {
        Path::new(&soundpack_dir(soundpack_id))
            .join("config.json")
            .to_string_lossy()
            .to_string()
    }
}