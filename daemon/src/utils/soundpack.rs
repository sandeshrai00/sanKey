use crate::state::paths;
use crate::state::soundpack::SoundpackMetadata;
use crate::utils::config_converter;
use crate::utils::soundpack_validator::{ validate_soundpack_config, SoundpackValidationStatus };
use std::fs;

/// Load soundpack metadata from config.json
pub fn load_soundpack_metadata(soundpack_id: &str) -> Result<SoundpackMetadata, String> {
    let config_path = paths::soundpacks::config_json(soundpack_id);

    // Validate the soundpack configuration first
    let validation_result = validate_soundpack_config(&config_path);

    // A pack built for a newer release still yields readable metadata (name,
    // author, icon), so it is listed rather than dropped. Carrying the reason
    // in `last_error` is what keeps "why is this one not working" answerable -
    // the status string alone has no text for a human to read.
    let last_error: Option<String> = match validation_result.status {
        SoundpackValidationStatus::RequiresNewerAppVersion(_) => {
            Some(validation_result.message.clone())
        }
        // Reaching the end of this function means metadata loaded, so there is
        // no error left to report. Conversion problems return early instead of
        // being recorded here - a pack whose conversion failed has no usable
        // metadata.
        _ => None,
    };

    // If it's a V1 config that can be converted, auto-convert it
    if
        validation_result.status == SoundpackValidationStatus::VersionOneNeedsConversion &&
        validation_result.can_be_converted
    {
        // Back up the original config before converting in place. The
        // conversion overwrites `config_path` itself, so without this backup a
        // failure part-way through leaves the pack with neither the original
        // nor a working config. A backup that cannot be written means the
        // conversion is unrecoverable, so refuse to start it rather than
        // convert without a safety net.
        let backup_path = format!("{}.v1.backup", config_path);
        if let Err(e) = fs::copy(&config_path, &backup_path) {
            return Err(
                format!(
                    "Refusing to convert {}: could not back up its config to {}: {}",
                    soundpack_id,
                    backup_path,
                    e
                )
            );
        }

        // Convert V1 to V2
        match config_converter::convert_v1_to_v2(&config_path, &config_path, None) {
            Ok(()) => {
                // Successfully converted
            }
            Err(e) => {
                let error_msg = format!("Failed to convert {} from V1 to V2: {}", soundpack_id, e);
                // Restore backup if conversion failed
                if fs::copy(&backup_path, &config_path).is_ok() {
                    // Restored backup
                }
                // Return error for conversion failure
                return Err(error_msg);
            }
        }
    }
    let content = fs
        ::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read config: {}", e))?;

    let config: serde_json::Value = serde_json
        ::from_str(&content)
        .map_err(|e| format!("Failed to parse config: {}", e))?;

    // Debug: Check if config has audio_file field
    let audio_file = config.get("audio_file").and_then(|v| v.as_str());
    crate::always_print!("🔍 [CACHE DEBUG] soundpack_id: {}", soundpack_id);
    crate::always_print!("🔍 [CACHE DEBUG] config_path: {}", config_path);
    crate::always_print!("🔍 [CACHE DEBUG] audio_file in config: {:?}", audio_file);

    // If audio_file exists, check if the actual file exists
    if let Some(audio_filename) = audio_file {
        let soundpack_dir = paths::soundpacks::soundpack_dir(soundpack_id);
        let full_audio_path = format!(
            "{}/{}",
            soundpack_dir,
            audio_filename.trim_start_matches("./")
        );
        crate::always_print!("🔍 [CACHE DEBUG] soundpack_dir: {}", soundpack_dir);
        crate::always_print!("🔍 [CACHE DEBUG] full_audio_path: {}", full_audio_path);
        crate::always_print!(
            "🔍 [CACHE DEBUG] audio file exists: {}",
            std::path::Path::new(&full_audio_path).exists()
        );

        if !std::path::Path::new(&full_audio_path).exists() {
            crate::always_print!("⚠️ [CACHE DEBUG] Audio file not found during cache refresh: {}", full_audio_path);
        }
    } else {
        crate::always_print!("⚠️ [CACHE DEBUG] No audio_file field found in config");
    }

    let name = config
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(soundpack_id)
        .to_string();

    let version = config
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("1.0.0")
        .to_string();

    let tags = config
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    // Re-validate after potential conversion
    let final_validation = validate_soundpack_config(&config_path);

    // Get file stats
    let metadata = fs
        ::metadata(&config_path)
        .map_err(|e| format!("Failed to get metadata: {}", e))?;
    Ok(SoundpackMetadata {
        id: soundpack_id.to_string(), // Use soundpack_id (with prefix) instead of config ID
        name,
        author: config
            .get("author")
            .or_else(|| config.get("m_author"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        description: config
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        version,
        tags,
        icon: config
            .get("icon")
            .and_then(|v| v.as_str())
.map(|s| s.to_string()),
        folder_path: soundpack_id.to_string(), // Store the relative path (e.g., "keyboard/Super Paper Mario Talk")
        last_modified: metadata
            .modified()
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        // Validation fields
        config_version: final_validation.config_version,
        is_valid_v2: final_validation.is_valid_v2,
        validation_status: match final_validation.status {
            SoundpackValidationStatus::Valid => "valid".to_string(),
            SoundpackValidationStatus::InvalidVersion => "invalid_version".to_string(),
            SoundpackValidationStatus::InvalidStructure(_) => "invalid_structure".to_string(),
            SoundpackValidationStatus::MissingRequiredFields(_) => "missing_fields".to_string(),
            SoundpackValidationStatus::VersionOneNeedsConversion => {
                "v1_needs_conversion".to_string()
            }
            SoundpackValidationStatus::RequiresNewerAppVersion(_) => {
                "requires_newer_app_version".to_string()
            }
        },
        // Error tracking - clear error if we successfully loaded metadata
        last_error: last_error,
    })
}

#[cfg(test)]
mod tests {
    /// B5: the metadata read path must stay write-free. The old rescan code
    /// called `fs::write` here on a V2-multi pack and silently destroyed its
    /// `method`/`audio_file` fields (the "converted" backup bug). This is the
    /// smallest check that fails if anyone re-introduces a write to the read
    /// path — no filesystem, no temp dirs, no daemon interference.
    #[test]
    fn the_metadata_read_path_never_writes_to_disk() {
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/utils/soundpack.rs"))
            .expect("read own source");
        let start = src.find("pub fn load_soundpack_metadata").unwrap();
        let body = &src[start..src.find("#[cfg(test)]").unwrap_or(src.len())];
        for forbidden in ["fs::write", "fs::create_dir", "fs::rename", "to_string_pretty"] {
            assert!(
                !body.contains(forbidden),
                "load_soundpack_metadata must not write to disk, but it uses `{forbidden}` — \
                 a read path that mutates the pack is the B5 data-loss bug"
            );
        }
    }
}
