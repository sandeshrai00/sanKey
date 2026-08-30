use crate::debug_print;
use crate::state::paths;
use crate::utils::{ data, path };
use chrono::{ DateTime, Utc };
use serde::{ Deserialize, Serialize };

/// Every field defaults independently, so one bad or absent entry costs the
/// user that single setting instead of the whole document. Without this a
/// hand-edited `"volume": "loud"` fails the entire parse, and the load path
/// then writes defaults over the file - theme, customizations and device
/// choices included.
///
/// `deserialize_lenient` additionally absorbs a wrong-typed *value* (`null`,
/// `[]`, a string where a number belongs), which a bare `#[serde(default)]`
/// does not: `default` covers a missing key, not a present-but-invalid one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppConfig {
    // Metadata
    pub version: String,
    pub last_updated: DateTime<Utc>,
    pub commit: Option<String>,
    // Audio settings
    pub keyboard_soundpack: String,
    pub volume: f32,
    pub enable_sound: bool,
    pub enable_keyboard_sound: bool,
    // Device settings
    pub selected_audio_device: Option<String>,
    // System settings
    pub auto_start: bool,
}

/// Parse a config document, discarding only the entries that cannot be read.
///
/// Each key is tried on its own against a default document; one that fails to
/// deserialize is dropped, and `#[serde(default)]` on the struct then fills
/// that field from `AppConfig::default()`. This matters over a per-field
/// `unwrap_or_default()`: the fallback for a damaged `volume` has to be the
/// config's 1.0, not `f32::default()`, which would silently leave the app
/// inaudible.
///
/// Only a document that is not a JSON object at all - truncated, or not JSON -
/// still fails here, which is what routes the caller to the preserve path.
pub fn parse_lenient(contents: &str) -> Result<AppConfig, String> {
    let value: serde_json::Value = serde_json
        ::from_str(contents)
        .map_err(|e| format!("not valid JSON: {}", e))?;

    let Some(object) = value.as_object() else {
        return Err("config is not a JSON object".to_string());
    };

    // Probe each entry against an otherwise-default document, so a key is
    // judged only on its own merits and cannot be failed by a bad neighbour.
    let mut accepted = serde_json::Map::new();
    for (key, entry) in object {
        // A null soundpack reads as "no pack" rather than as a damaged entry:
        // "" is the engine's supported no-sound state, so honouring the user's
        // null is closer to their intent than restoring the default pack.
        let entry = if
            entry.is_null() &&
            key.as_str() == "keyboard_soundpack"
        {
            serde_json::Value::String(String::new())
        } else {
            entry.clone()
        };

        let mut probe = serde_json::Map::new();
        probe.insert(key.clone(), entry.clone());

        if serde_json::from_value::<AppConfig>(serde_json::Value::Object(probe)).is_ok() {
            accepted.insert(key.clone(), entry);
        } else {
            crate::always_eprint!("⚠️  Ignoring unusable config entry '{}', using its default", key);
        }
    }

    serde_json
        ::from_value(serde_json::Value::Object(accepted))
        .map_err(|e| format!("config could not be rebuilt: {}", e))
}

/// Outcome of trying to move an unparseable config out of the way.
enum Preserved {
    /// There was no file to preserve - a first run, or it was deleted.
    Nothing,
    /// The user's bytes now live at this path and are safe to overwrite from.
    MovedTo(std::path::PathBuf),
    /// The file is still in place and must not be written over.
    Failed(String),
}

/// Move an unparseable config aside so its bytes survive, returning where it
/// went.
///
/// Renaming rather than copying means the original path is free for a fresh
/// default file without a window where both are half-written. An existing
/// `.corrupt` from an earlier failure is never clobbered - the first failure
/// usually holds the settings worth recovering, so later ones get a numbered
/// suffix instead.
fn preserve_corrupt_config(config_path: &std::path::Path) -> Preserved {
    if !config_path.exists() {
        return Preserved::Nothing;
    }

    let base = {
        let mut name = config_path.as_os_str().to_os_string();
        name.push(".corrupt");
        std::path::PathBuf::from(name)
    };

    // First failure gets the plain `.corrupt` name; later ones are numbered so
    // no earlier rescue is overwritten.
    let mut target = base.clone();
    let mut attempt = 1;
    while target.exists() {
        let mut name = base.as_os_str().to_os_string();
        name.push(format!(".{}", attempt));
        target = std::path::PathBuf::from(name);
        attempt += 1;

        // Refuse to spin forever if something is generating these.
        if attempt > 100 {
            return Preserved::Failed(
                "too many saved copies of a damaged config already exist".to_string()
            );
        }
    }

    match std::fs::rename(config_path, &target) {
        Ok(()) => Preserved::MovedTo(target),
        Err(e) => Preserved::Failed(e.to_string()),
    }
}

impl AppConfig {
    /// Check if config data has changed (excluding metadata fields)
    pub fn data_equals(&self, other: &Self) -> bool {
        self.keyboard_soundpack == other.keyboard_soundpack
            && self.volume == other.volume
            && self.enable_sound == other.enable_sound
            && self.enable_keyboard_sound == other.enable_keyboard_sound
            && self.selected_audio_device == other.selected_audio_device
            && self.auto_start == other.auto_start
    }

    pub fn load() -> Self {
        let config_path = paths::data::config_json();

        // Ensure data directory exists
        if let Some(parent) = config_path.parent() {
            if let Err(_) = path::ensure_directory_exists(parent) {
                crate::always_eprint!("⚠️  Could not create data directory");
            }
        }

        debug_print!("📖 Loading config from: {}", config_path.display());

        // Read and parse separately: a read failure (first run) and a parse
        // failure (damaged document) need different handling, and only the
        // latter has bytes worth preserving.
        let parsed = std::fs
            ::read_to_string(&config_path)
            .map_err(|e| format!("could not read '{}': {}", config_path.display(), e))
            .and_then(|contents| parse_lenient(&contents));

        match parsed {
            Ok(mut config) => {
                let mut config_updated = false;

                // Migrate old soundpack IDs to new Sorakey names
                let migrate = |old: &str, new: &str| (old.to_string(), new.to_string());
                let renames = [
                    migrate("oreo", "keyboard/sankey-oreo"),
                    migrate("keyboard/eg-oreo", "keyboard/sankey-oreo"),
                    migrate("keyboard/eg-crystal-purple", "keyboard/sankey-crystal-purple"),
                    migrate("keyboard/cherrymx-black-abs", "keyboard/sankey-mx-black-abs"),
                    migrate("keyboard/cherrymx-black-pbt", "keyboard/sankey-mx-black-pbt"),
                    migrate("keyboard/cherrymx-blue-abs", "keyboard/sankey-mx-blue-abs"),
                    migrate("keyboard/cherrymx-blue-pbt", "keyboard/sankey-mx-blue-pbt"),
                    migrate("keyboard/cherrymx-brown-abs", "keyboard/sankey-mx-brown-abs"),
                    migrate("keyboard/cherrymx-brown-pbt", "keyboard/sankey-mx-brown-pbt"),
                    migrate("keyboard/cherrymx-red-abs", "keyboard/sankey-mx-red-abs"),
                    migrate("keyboard/cherrymx-red-pbt", "keyboard/sankey-mx-red-abs"),
                    migrate("keyboard/topre-purple-hybrid-pbt", "keyboard/sankey-topre-purple"),
                ];
                for (old, new) in renames {
                    if config.keyboard_soundpack == old {
                        crate::always_print!("🔄 Migrating keyboard soundpack: {} -> {}", old, new);
                        config.keyboard_soundpack = new;
                        config_updated = true;
                        break;
                    }
                }

                // Drop index-based audio device IDs, reverting to the system
                // default.
                //
                // `output_{index}` resolved by enumeration position, which is
                // not an identity: unplugging any device shifts everything
                // after it down a slot, so the saved index may already point at
                // a different device than the one the user picked. Resolving it
                // one last time would only launder that guess into a
                // permanent-looking name-based ID, so a selection that can no
                // longer be trusted is discarded instead. The user picks again,
                // and from then on the name-based ID stays put.
                //
                // Deliberately does not enumerate devices: that costs hundreds
                // of ms on a path that runs during `config_writer`'s
                // `get_or_init`. Nothing reachable from here may call
                // `config_writer::current()` either - that would re-enter the
                // initialising `OnceLock` and deadlock.
                if let Some(device_id) = config.selected_audio_device.clone() {
                    if crate::libs::device_manager::is_legacy_index_device_id(&device_id) {
                        crate::always_print!(
                            "🔄 Dropping index-based audio device {}: reverting to system default",
                            device_id
                        );
                        config.selected_audio_device = None;
                        config_updated = true;
                    }
                }

                // Migrate default volume from 1.0 (100%) to 0.6 (60%)
                if config.volume == 1.0 {
                    crate::always_print!("🔄 Migrating default volume: 1.0 → 0.6");
                    config.volume = 0.6;
                    config_updated = true;
                }

                // Sync auto_start with actual registry state
                let actual_auto_start = crate::utils::auto_startup::get_auto_startup_state();
                if config.auto_start != actual_auto_start {
                    crate::always_print!(
                        "🔄 Syncing auto_start config with registry: {} -> {}",
                        config.auto_start,
                        actual_auto_start
                    );
                    config.auto_start = actual_auto_start;
                    config_updated = true;
                }

                // Save if any migrations were applied
                if config_updated {
                    config.last_updated = chrono::Utc::now();
                    let _ = config.save();
                }

                config
            }
            Err(e) => {
                // Per-field tolerance above means reaching here needs the
                // document itself to be unreadable (truncated, not JSON, or
                // unreadable from disk) - not merely one bad setting.
                crate::always_eprint!("❌ Failed to load config file: {}", e);
                crate::always_eprint!("   Config path: {}", config_path.display());

                // Never overwrite the user's file in place: it is the only
                // copy of their theme, customizations and device choices, and
                // a hand-editing slip is recoverable only while those bytes
                // still exist. Move it aside first, and if it cannot be moved,
                // do not write defaults over it at all.
                match preserve_corrupt_config(&config_path) {
                    Preserved::Nothing => {
                        // No file to lose (first run, or it was deleted).
                        let default_config = Self::default();
                        let _ = default_config.save();
                        default_config
                    }
                    Preserved::MovedTo(backup) => {
                        crate::always_eprint!(
                            "   Your previous config was kept at: {}",
                            backup.display()
                        );
                        crate::always_eprint!("   Defaults are in use for this session.");
                        let default_config = Self::default();
                        let _ = default_config.save();
                        default_config
                    }
                    Preserved::Failed(err) => {
                        crate::always_eprint!("   Could not set the damaged config aside: {}", err);
                        crate::always_eprint!(
                            "   Leaving it untouched and running on defaults; \
                             settings changed now will not be saved over it."
                        );
                        Self::default()
                    }
                }
            }
        }
    }

    /// Write this struct over `config.json`, replacing every field.
    ///
    /// Deliberately **not** public. Reaching this from a subsystem means
    /// holding an `AppConfig` across time and writing it back, which reverts
    /// whatever another subsystem changed in between - the defect that shipped
    /// three times from three unrelated call sites. `config_writer::apply` is
    /// the only caller outside this module's own load path, and it always
    /// writes state it owns and has just mutated.
    pub(in crate::state) fn save(&self) -> Result<(), String> {
        let config_path = paths::data::config_json();
        debug_print!("💾 Saving config to: {}", config_path.display());
        debug_print!("   keyboard_soundpack: {}", self.keyboard_soundpack);
        data::save_json_to_file_atomically(self, &config_path)
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: crate::utils::constants::APP_VERSION.to_string(),
            last_updated: Utc::now(),
            commit: option_env!("GIT_HASH").map(|s| s.to_string()),
            keyboard_soundpack: "keyboard/sankey-oreo".to_string(),
            volume: 0.6,
            enable_sound: true,
            enable_keyboard_sound: true,
            selected_audio_device: None,
            auto_start: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Why `save()` is `pub(in crate::state)` and `config_writer::apply` is the
    /// only public way to write.
    ///
    /// `save()` rewrites the entire struct, so a writer that mutates a copy it
    /// captured earlier reverts every field someone else changed in between.
    /// This is the mute bug: the volume path held a config from before the mute
    /// click and wrote `enable_sound: true` back over it. The shape is
    /// reproduced here on plain structs because it can no longer be expressed
    /// against the real API - there is no public function that accepts an
    /// `AppConfig` to persist, so a caller has nothing to hold and write back.
    #[test]
    fn holding_a_struct_and_writing_it_back_is_what_reverts_a_concurrent_change() {
        let on_disk = AppConfig::default();

        // Volume path reads the config...
        let mut volume_writer_copy = on_disk.clone();

        // ...then the user mutes, and that write lands on disk first.
        let mut after_mute = on_disk.clone();
        after_mute.enable_sound = false;

        // Volume path now saves the copy it captured before the mute.
        volume_writer_copy.volume = 0.5;
        let persisted = volume_writer_copy;

        assert!(!after_mute.enable_sound, "mute was applied to disk");
        assert!(
            persisted.enable_sound,
            "stale save silently restores enable_sound - which is why no public \
             API accepts a config to write"
        );
    }

    /// load path resets *every* setting on a deserialize error, so a missing
    /// `#[serde(default)]` here would wipe the user's whole config on upgrade.
    #[test]

    /// `data_equals` drives whether a change is persisted at all. A field
    /// missing from it means the Settings toggle appears to work and is
    /// silently forgotten on restart.
    #[test]

    /// The fix: re-read immediately before mutating, so the concurrent change
    /// is already present in the struct that gets written back.
    #[test]
    fn reloading_before_mutating_preserves_a_concurrent_change() {
        let mut on_disk = AppConfig::default();

        // User mutes first.
        on_disk.enable_sound = false;

        // Volume path re-reads *now* rather than reusing an older copy.
        let mut fresh = on_disk.clone();
        fresh.volume = 0.5;

        assert!(!fresh.enable_sound, "mute must survive an unrelated config write");
        assert_eq!(fresh.volume, 0.5, "the volume change must still apply");
    }

    /// Build a config document from the defaults, then apply edits, the way a
    /// user hand-editing `config.json` would.
    fn config_json_with(edits: &[(&str, serde_json::Value)]) -> String {
        let mut value = serde_json::to_value(AppConfig::default()).expect("config serializes");
        let object = value.as_object_mut().expect("config is a json object");
        for (key, entry) in edits {
            object.insert((*key).to_string(), entry.clone());
        }
        value.to_string()
    }

    /// A null where a string belongs used to fail the whole document, which
    /// sent the load path down the arm that wrote defaults over the file.
    /// It must now cost only that one field.
    #[test]
    fn a_null_soundpack_costs_only_that_field() {
        let document = config_json_with(
            &[
                ("keyboard_soundpack", serde_json::Value::Null),
                ("volume", serde_json::json!(0.42)),
                ("auto_start", serde_json::json!(true)),
            ]
        );

        assert!(
            serde_json::from_str::<AppConfig>(&document).is_err(),
            "a null in a non-optional field must really break a strict parse, \
             or this test is not exercising the bug"
        );

        let restored = parse_lenient(&document).expect("a null field must not fail the document");

        assert_eq!(restored.keyboard_soundpack, "", "null must read as the no-pack state");
        assert_eq!(restored.volume, 0.42, "every other setting must survive intact");
        assert!(restored.auto_start, "and so must the rest");
    }

    /// The other shapes a hand-edit produces: an array or a string where a
    /// number belongs, and an outright unknown key. None may cost more than
    /// the field it appears on - and a dropped field must fall back to the
    /// config's own default, not the type's.
    #[test]
    fn wrong_typed_and_unknown_fields_do_not_fail_the_document() {
        let document = config_json_with(
            &[
                ("volume", serde_json::json!("loud")),
                ("auto_start", serde_json::json!(true)),
                ("a_key_from_a_future_build", serde_json::json!({ "x": 1 })),
            ]
        );

        let restored = parse_lenient(&document).expect(
            "wrong-typed fields must not fail the document"
        );

        assert_eq!(
            restored.volume,
            AppConfig::default().volume,
            "a damaged volume must fall back to the audible config default, not 0.0"
        );
        assert!(restored.auto_start, "a valid neighbouring setting must survive");
    }

    /// A missing field is the upgrade case: an older config lacking a newer
    /// key must load with that key defaulted, not reset everything.
    #[test]
    fn a_missing_field_defaults_without_losing_the_rest() {
        let mut value = serde_json::to_value(AppConfig::default()).expect("config serializes");
        let object = value.as_object_mut().expect("config is a json object");
        object.remove("auto_start");
        object.insert("volume".to_string(), serde_json::json!(0.8));

        let restored = parse_lenient(&value.to_string()).expect(
            "a missing field must default rather than fail"
        );

        assert!(!restored.auto_start, "the absent field takes its default");
        assert_eq!(
            restored.volume,
            0.8,
            "the user's other settings must be untouched"
        );
    }

    /// An empty array in place of the whole document, and other non-object
    /// shapes, cannot be salvaged field-by-field - they must be reported as a
    /// parse failure so the caller preserves the file rather than parsing it
    /// into silent defaults.
    #[test]
    fn a_non_object_document_is_a_parse_failure() {
        assert!(parse_lenient("[]").is_err(), "a bare array is not a config");
        assert!(parse_lenient("\"hello\"").is_err(), "a bare string is not a config");
        assert!(parse_lenient("{\"volume\": 0.5").is_err(), "a truncated document must fail");
    }

    /// The destructive case the fix targets: a document too damaged to parse
    /// at all. The user's bytes must end up in `.corrupt`, and the original
    /// path must not still hold them - it is replaced by a fresh default.
    #[test]
    fn a_truncated_config_is_preserved_rather_than_overwritten() {
        let dir = std::env::temp_dir().join(format!("sorakey-corrupt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let config_path = dir.join("config.json");

        let original = br#"{"volume": 0.7, "theme": "#.to_vec();
        std::fs::write(&config_path, &original).expect("write truncated config");

        assert!(
            serde_json::from_slice::<AppConfig>(&original).is_err(),
            "this fixture must really be unparseable, or the test proves nothing"
        );

        let preserved = preserve_corrupt_config(&config_path);
        let backup = match preserved {
            Preserved::MovedTo(path) => path,
            _ => panic!("an existing damaged config must be preserved"),
        };

        assert_eq!(
            std::fs::read(&backup).expect("backup readable"),
            original,
            "the user's original bytes must survive verbatim"
        );
        assert!(!config_path.exists(), "the damaged file is moved, not copied");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// A second failure must not erase the first rescue, which is usually the
    /// one still holding the user's real settings.
    #[test]
    fn a_second_failure_does_not_clobber_the_first_rescue() {
        let dir = std::env::temp_dir().join(
            format!("sorakey-corrupt-twice-{}", std::process::id())
        );
        std::fs::create_dir_all(&dir).expect("temp dir");
        let config_path = dir.join("config.json");

        std::fs::write(&config_path, b"first damaged config").expect("write");
        let first = match preserve_corrupt_config(&config_path) {
            Preserved::MovedTo(path) => path,
            _ => panic!("first preservation must succeed"),
        };

        std::fs::write(&config_path, b"second damaged config").expect("write");
        let second = match preserve_corrupt_config(&config_path) {
            Preserved::MovedTo(path) => path,
            _ => panic!("second preservation must succeed"),
        };

        assert_ne!(first, second, "the second rescue must take a distinct path");
        assert_eq!(
            std::fs::read(&first).expect("first backup readable"),
            b"first damaged config",
            "the earlier rescue must still hold its original bytes"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Nothing on disk means nothing to rescue - writing defaults there is
    /// correct and must not be mistaken for a failure.
    #[test]
    fn a_missing_config_file_is_not_treated_as_a_rescue() {
        let dir = std::env::temp_dir().join(format!("sorakey-absent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");

        let preserved = preserve_corrupt_config(&dir.join("config.json"));
        assert!(
            matches!(preserved, Preserved::Nothing),
            "an absent config is a first run, not a rescue"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Guards the "skip redundant writes" half of the fix: re-asserting a value
    /// that already matches must not count as a change, or mount effects will
    /// rewrite the config continuously.
    #[test]
    fn reasserting_an_identical_value_is_not_a_change() {
        let config = AppConfig::default();
        let mut same = config.clone();
        same.volume = config.volume;

        assert!(same.data_equals(&config), "no-op write must not be treated as a change");

        same.volume = config.volume + 0.25;
        assert!(!same.data_equals(&config), "a real change must still be detected");
    }

    /// A device saved as `output_{index}` cannot be trusted: the index is a
    /// position in the enumeration, so unplugging anything ahead of it makes
    /// it point somewhere else. Config load drops it rather than resolving it,
    /// which would only launder the guess into a permanent-looking id.
    #[test]
    fn an_index_based_device_id_is_recognised_as_legacy_and_droppable() {
        use crate::libs::device_manager::is_legacy_index_device_id;

        assert!(is_legacy_index_device_id("output_2"));
        assert!(is_legacy_index_device_id("output_0"));

        // Name-based ids and the default sentinel survive: they are stable, so
        // there is nothing to drop.
        assert!(!is_legacy_index_device_id("output_name:0b3a976597860826"));
        assert!(!is_legacy_index_device_id("output_default"));
    }
}
