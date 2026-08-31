use crate::state::paths;
use crate::state::soundpack::SoundPack;
use crate::state::soundpack::{ SoundpackCache, SoundpackMetadata };
use std::sync::Arc;

use super::engine::EngineState;

/// (samples, channels, sample_rate) for a decoded buffer.
type DecodedAudio = (Arc<Vec<f32>>, u16, u32);

/// Loads and decodes a soundpack's audio file, then resamples it to
/// `device_rate` if given. Returns `(original, resampled)`; `original` keeps
/// the file's native rate so a later device switch can re-resample without
/// re-reading from disk.
fn load_audio_file(
    soundpack_path: &str,
    soundpack: &SoundPack,
    device_rate: Option<u32>
) -> Result<(DecodedAudio, DecodedAudio), String> {
    let audio_file = soundpack.audio_file.as_ref()
        .ok_or_else(|| "No audio_file field in soundpack config".to_string())?;
    let sanitized = audio_file.trim_start_matches("./").replace('\\', "/");
    if sanitized.contains("..") || sanitized.contains('\0') || sanitized.starts_with('/') {
        return Err(format!("Invalid audio_file path: {}", audio_file));
    }
    let sound_file_path = format!("{}/{}", soundpack_path, sanitized);
    load_audio_file_for_path(&sound_file_path, device_rate)
}

/// Like `load_audio_file` but takes an explicit file path instead of reading
/// `soundpack.audio_file`. Used for loading per-key audio files in multi-method
/// packs.
fn load_audio_file_for_path(
    sound_file_path: &str,
    device_rate: Option<u32>
) -> Result<(DecodedAudio, DecodedAudio), String> {
    if !std::path::Path::new(sound_file_path).exists() {
        return Err(format!("Sound file not found: {}", sound_file_path));
    }

    let (samples, channels, file_rate) = load_audio_with_symphonia(sound_file_path).map_err(
        |e| format!("Failed to load audio: {}", e)
    )?;

    match device_rate {
        Some(device_rate) if device_rate != file_rate => {
            let start = std::time::Instant::now();
            let resampled = super::resampler::resample_interleaved(
                &samples,
                channels,
                file_rate,
                device_rate
            );
            crate::always_print!(
                "🔁 Resampled soundpack audio {}Hz -> {}Hz in {:.1}ms (Cubic 64/32 0.95)",
                file_rate,
                device_rate,
                start.elapsed().as_secs_f64() * 1000.0
            );
            let original = (Arc::new(samples), channels, file_rate);
            Ok((original, (Arc::new(resampled), channels, device_rate)))
        }
        _ => {
            let shared = Arc::new(samples);
            Ok(((shared.clone(), channels, file_rate), (shared, channels, file_rate)))
        }
    }
}

fn load_audio_with_symphonia(file_path: &str) -> Result<(Vec<f32>, u16, u32), String> {
    let meta = std::fs::metadata(file_path).map_err(|e| format!("Failed to get file metadata: {}", e))?;
    if meta.len() == 0 {
        return Err(format!("Audio file is empty: {}", file_path));
    }
    crate::utils::symphonia::decode_interleaved(file_path)
}

/// Derives the `type/name` id the cache is keyed by from a pack's absolute
/// path against the single soundpacks root. Kept separate from the root
/// itself so the path logic can be tested without a filesystem.
fn soundpack_id_from_path(soundpack_path: &str) -> String {
    let roots = [crate::utils::path::get_soundpacks_dir_absolute()];
    relative_soundpack_id(soundpack_path, &roots)
}

/// The id for `soundpack_path` relative to whichever of `roots` contains it.
///
/// Falls back to the last two path components rather than the folder name
/// alone: an id without its `keyboard/`/`mouse/` prefix does not match the key
/// the directory scanner uses, and inserting under it duplicates the pack in
/// the cache and in every list rendered from it.
fn relative_soundpack_id(soundpack_path: &str, roots: &[String]) -> String {
    let path = std::path::Path::new(soundpack_path);

    for root in roots {
        if let Ok(relative) = path.strip_prefix(root) {
            return relative.to_string_lossy().replace('\\', "/");
        }
    }

    let mut tail: Vec<&str> = path
        .components()
        .rev()
        .take(2)
        .filter_map(|component| component.as_os_str().to_str())
        .collect();
    tail.reverse();

    if tail.is_empty() { "unknown".to_string() } else { tail.join("/") }
}

fn create_soundpack_metadata(
    soundpack_path: &str,
    soundpack: &SoundPack
) -> Result<SoundpackMetadata, String> {
    // Extract the soundpack ID from the full path
    // e.g., "/path/to/soundpacks/keyboard/Apex by teia" -> "keyboard/Apex by teia"
    //
    // Both roots have to be tried. Packs live under either the bundled
    // directory or the custom one in app data, and the id has to come out as
    // `type/name` for either: the scanner keys the cache that way, so an id
    // that loses its `keyboard/` prefix here inserts a second entry for a pack
    // already in the cache and the UI lists it twice. That is why only
    // imported packs duplicated - bundled ones matched the first root and
    // never reached the fallback.
    let id = soundpack_id_from_path(soundpack_path);

    // Get file metadata
    let last_modified = match std::fs::metadata(soundpack_path) {
        Ok(metadata) =>
            metadata
                .modified()
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        Err(_) => 0,
    };

    Ok(SoundpackMetadata {
        id: id.clone(), // Use calculated relative path ID instead of config ID
        name: soundpack.name.clone(),
        author: soundpack.author.clone(),
        description: soundpack.description.clone(),
        version: soundpack.version.clone().unwrap_or_else(|| "1.0".to_string()),
        tags: soundpack.tags.clone().unwrap_or_default(),
        icon: {
            // Generate dynamic URL for icon instead of base64 conversion
            if let Some(icon_filename) = &soundpack.icon {
                let icon_path = format!("{}/{}", soundpack_path, icon_filename);
                if std::path::Path::new(&icon_path).exists() {
                    // Create dynamic URL that will be served by the asset handler
                    Some(format!("/soundpack-images/{}/{}", id, icon_filename))
                } else {
                    Some(String::new()) // Empty string if icon file not found
                }
            } else {
                Some(String::new()) // Empty string if no icon specified
            }
        },
        soundpack_type: soundpack.soundpack_type.clone(), 
        folder_path: id, // Use the derived folder path for loading
        last_modified,
        last_accessed: std::time::SystemTime
            ::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(), // Add validation fields with default values
        config_version: Some(soundpack.config_version_num),
        is_valid_v2: true, // Assume valid since it loaded successfully
        validation_status: "valid".to_string(),
        can_be_converted: false,
        // Error tracking - None since we successfully created metadata
        last_error: None,
    })
}

fn create_key_mappings(
    soundpack: &SoundPack,
) -> std::collections::HashMap<String, Vec<(f64, f64)>> {
    let mut key_mappings = std::collections::HashMap::new();
    for (key, key_def) in &soundpack.definitions {
        let converted_mappings: Vec<(f64, f64)> = key_def.timing
            .iter()
            .map(|pair| (pair[0] as f64, pair[1] as f64))
            .collect();
        key_mappings.insert(key.clone(), converted_mappings);
    }
    key_mappings
}

/// Loads a keyboard soundpack directly into engine-owned state (Phase 3).
/// Mirrors `load_keyboard_soundpack_optimized` but writes to `EngineState`
/// fields instead of an `&AudioContext`, since the engine thread owns its
/// data as plain fields rather than `Arc<Mutex<...>>`.
pub(super) fn load_keyboard_pack_into_engine(
    state: &mut EngineState,
    soundpack_id: &str,
    update_cache_on_error: bool
) -> Result<String, String> {
    if soundpack_id.is_empty() {
        return Err("empty soundpack ID".to_string());
    }

    match load_keyboard_pack_into_engine_inner(state, soundpack_id) {
        Ok(name) => Ok(name),
        Err(e) => {
            if update_cache_on_error {
                capture_soundpack_loading_error(soundpack_id, &e);
            }
            Err(e)
        }
    }
}

fn load_keyboard_pack_into_engine_inner(
    state: &mut EngineState,
    soundpack_id: &str
) -> Result<String, String> {
    // free old audio first so peak stays low
    let _old_kb = state.keyboard_samples.take();
    let _old_orig = state.keyboard_samples_original.take();
    drop(_old_kb);
    drop(_old_orig);
    state.multi_key_audio.clear();
    state.multi_key_audio_map.clear();
    let soundpack_path = paths::soundpacks::soundpack_dir(soundpack_id);
    let config_path = paths::soundpacks::config_json(soundpack_id);
    let config_content = std::fs
        ::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read config: {}", e))?;
    let soundpack: SoundPack = serde_json
        ::from_str(&config_content)
        .map_err(|e| format!("Failed to parse V2 soundpack config: {}", e))?;

    if soundpack.definition_method == "multi" {
        // Multi-method: load each unique per-key audio file once, cache them.
        // Build map: key -> audio_file and collect unique audio files.
        let mut key_audio_file: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let mut unique_audio_files: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (key, key_def) in &soundpack.definitions {
            if let Some(audio_file) = &key_def.audio_file {
                key_audio_file.insert(key.clone(), audio_file.clone());
                unique_audio_files.insert(audio_file.clone());
            }
        }

        // Load each unique audio file once
        let mut multi_key_audio: std::collections::HashMap<String, super::engine::MultiKeyAudio>
            = std::collections::HashMap::new();
        for audio_file in unique_audio_files {
            let sanitized = audio_file.trim_start_matches("./").replace('\\', "/");
            if sanitized.contains("..") || sanitized.contains('\0') || sanitized.starts_with('/') {
                crate::always_eprint!("⚠️ [Engine] Skipping invalid per-key audio path '{}'", audio_file);
                continue;
            }
            let file_path = format!("{}/{}", soundpack_path, sanitized);
            match load_audio_file_for_path(&file_path, state.device_rate) {
                Ok((_original, resampled)) => {
                    let (samples, channels, sample_rate) = resampled;
                    multi_key_audio.insert(audio_file.clone(), super::engine::MultiKeyAudio {
                        samples,
                        channels,
                        sample_rate,
                    });
                    crate::always_print!(
                        "✅ [Engine] Loaded multi-method audio file '{}' ({}Hz, {}ch)",
                        audio_file, sample_rate, channels
                    );
                }
                Err(e) => {
                    crate::always_eprint!(
                        "⚠️ [Engine] Failed to load per-key audio '{}': {}",
                        audio_file, e
                    );
                }
            }
        }

        // Build key_mappings from definitions
        let key_mappings = create_key_mappings(&soundpack);

        state.key_map.clear();
        for (key, mappings) in key_mappings {
            let converted: Vec<[f32; 2]> = mappings
                .into_iter()
                .map(|(start, end)| [start as f32, end as f32])
                .collect();
            state.key_map.insert(key.clone(), converted);
        }

        state.multi_key_audio = multi_key_audio;
        state.multi_key_audio_map = key_audio_file;
        state.keyboard_samples = None;
        state.keyboard_samples_original = None;
    } else {
        // Single-method: load the shared audio file
        let (original, resampled) = load_audio_file(&soundpack_path, &soundpack, state.device_rate)?;
        let key_mappings = create_key_mappings(&soundpack);

        let (audio_samples, channels, sample_rate) = resampled;
        state.keyboard_samples = Some((audio_samples, channels, sample_rate));
        let (orig_samples, orig_channels, orig_rate) = original;
        state.keyboard_samples_original = Some((orig_samples, orig_channels, orig_rate));

        state.key_map.clear();
        for (key, mappings) in key_mappings {
            let converted: Vec<[f32; 2]> = mappings
                .into_iter()
                .map(|(start, end)| [start as f32, end as f32])
                .collect();
            state.key_map.insert(key, converted);
        }
    }

    state.key_sinks.clear();
    // give freed pages back to the OS
    unsafe { libc::malloc_trim(0); }

    update_soundpack_cache(&soundpack_path, &soundpack, soundpack_id);
    crate::always_print!("✅ [Engine] Loaded keyboard soundpack: {}", soundpack.name);
    Ok(soundpack.name)
}

/// Update the soundpack cache after a successful load.
fn update_soundpack_cache(soundpack_path: &str, soundpack: &SoundPack, soundpack_id: &str) {
    let mut cache = SoundpackCache::load();
    match create_soundpack_metadata(soundpack_path, soundpack) {
        Ok(metadata) => {
            cache.add_soundpack(metadata);
        }
        Err(e) => {
            crate::always_print!("⚠️ Failed to create metadata for {}: {}", soundpack_id, e);
        }
    }
    cache.save();
}

/// Capture soundpack loading error and update the cache
fn capture_soundpack_loading_error(soundpack_id: &str, error: &str) {
    // Skip creating cache entries for empty soundpack IDs
    if soundpack_id.is_empty() {
        crate::always_print!("⚠️ Skipping cache entry for empty soundpack ID: {}", error);
        return;
    }

    crate::always_print!("📝 Capturing loading error for {}: {}", soundpack_id, error);

    let mut cache = SoundpackCache::load();

    // Check if we already have metadata for this soundpack
    if let Some(existing_metadata) = cache.soundpacks.get_mut(soundpack_id) {
        // Update existing metadata with error
        existing_metadata.last_error = Some(error.to_string());
        existing_metadata.validation_status = "loading_error".to_string();
    } else {
        // Create minimal metadata entry with error information
        let error_metadata = SoundpackMetadata {
            id: soundpack_id.to_string(),
            name: format!("Error: {}", soundpack_id),
            author: None,
            description: Some(format!("Loading failed: {}", error)),
            version: "unknown".to_string(),
            tags: vec!["error".to_string()],
            icon: None,
            soundpack_type: crate::state::soundpack::SoundpackType::Keyboard,
            folder_path: soundpack_id.to_string(), // Add folder_path for error entries
            last_modified: 0,
            last_accessed: std::time::SystemTime
                ::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            config_version: None,
            is_valid_v2: false,
            validation_status: "loading_error".to_string(),
            can_be_converted: false,
            last_error: Some(error.to_string()),
        };

        const MAX_CACHE_ENTRIES: usize = 1000;
        if cache.soundpacks.len() >= MAX_CACHE_ENTRIES {
            if let Some(old) = cache.soundpacks.iter().find(|(_, m)| m.last_error.is_some()).map(|(k, _)| k.clone()) {
                cache.soundpacks.remove(&old);
            } else {
                crate::always_eprint!("⚠️ cache full ({}), skipping error entry for {}", cache.soundpacks.len(), soundpack_id);
                return;
            }
        }
        cache.soundpacks.insert(soundpack_id.to_string(), error_metadata);
    }

    cache.save();
    crate::always_print!("💾 Updated cache with error information for {}", soundpack_id);
}

#[cfg(test)]
mod tests {
    use super::relative_soundpack_id;

    /// Joins with the running platform's separator. `Path::components` only
    /// splits on the native one, so a hard-coded `\` is a single component on
    /// Linux and the assertions below would be testing nothing there.
    fn native_path(parts: &[&str]) -> String {
        parts.join(std::path::MAIN_SEPARATOR_STR)
    }

    fn builtin_root() -> String {
        native_path(&["", "opt", "Sorakey", "soundpacks"])
    }

    fn roots() -> Vec<String> {
        vec![builtin_root()]
    }

    #[test]
    fn an_imported_pack_gets_the_same_id_the_scanner_uses() {
        // Only one root now; the scanner keys the cache by `keyboard/name`.
        assert_eq!(
            relative_soundpack_id(
                &native_path(&[&builtin_root(), "keyboard", "eg-oreo"]),
                &roots()
            ),
            "keyboard/eg-oreo"
        );
    }

    #[test]
    fn a_path_under_no_known_root_still_keeps_its_type_prefix() {
        // The fallback keeps two components rather than one, so even an
        // unexpected location cannot produce a prefix-less id.
        assert_eq!(
            relative_soundpack_id(
                &native_path(&["", "elsewhere", "keyboard", "Model O"]),
                &roots()
            ),
            "keyboard/Model O"
        );
    }
}
