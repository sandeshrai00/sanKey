//! Fixed installation layout. Sorakey is always installed under
//! `~/.local/share/sorakey` - no AppImage, no system dirs, no ambiguity.
//!
//! ```text
//! ~/.local/share/sorakey/
//! ├── data/                      config.json, soundpack_cache.json, images/
//! └── soundpacks/                keyboard soundpacks (built-in + imported)
//! ```

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

    pub const BUILTIN_SOUNDPACKS: &[&str] = &[
        "keyboard/sankey-mx-black-abs",
        "keyboard/sankey-mx-black-pbt",
        "keyboard/sankey-mx-blue-abs",
        "keyboard/sankey-mx-blue-pbt",
        "keyboard/sankey-mx-brown-abs",
        "keyboard/sankey-mx-brown-pbt",
        "keyboard/sankey-mx-red-abs",
        "keyboard/sankey-crystal-purple",
        "keyboard/sankey-oreo",
        "keyboard/sankey-topre-purple",
    ];

    pub fn is_builtin_soundpack(soundpack_id: &str) -> bool {
        BUILTIN_SOUNDPACKS.contains(&soundpack_id)
    }

    pub fn get_builtin_soundpacks_dir() -> PathBuf {
        static DIR: OnceLock<PathBuf> = OnceLock::new();
        DIR.get_or_init(|| data_dir().join("soundpacks")).clone()
    }

    pub fn ensure_soundpack_directories() -> std::io::Result<()> {
        std::fs::create_dir_all(get_builtin_soundpacks_dir().join("keyboard"))?;
        Ok(())
    }

    /// Directory for a soundpack id (`"keyboard/Name"`). All packs, whether
    /// shipped in the repo or imported by the user, live in the same tree.
    pub fn soundpack_dir(soundpack_id: &str) -> String {
        let sanitized = soundpack_id.replace('\\', "/");
        let parts: Vec<&str> = sanitized.split('/').filter(|p| !p.is_empty() && *p != ".." && !p.contains('\0')).collect();
        let join = |base: &Path| -> PathBuf {
            parts.iter().fold(base.to_path_buf(), |p, part| p.join(part))
        };
        let base = get_builtin_soundpacks_dir();
        let joined = join(Path::new(&base));
        // Ensure result stays under base (defense in depth)
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

    pub fn get_soundpacks_dir() -> String {
        get_builtin_soundpacks_dir().to_string_lossy().to_string()
    }
}