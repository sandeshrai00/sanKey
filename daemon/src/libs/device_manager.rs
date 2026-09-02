use cpal::traits::{ DeviceTrait, HostTrait };
use cpal::{ Device, Host };

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

/// Stable device ID from name (FNV-1a hash). Survives replugs unlike index-based IDs.
fn device_id_from_name(prefix: &str, name: &str) -> String {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET;
    for byte in name.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    format!("{}_name:{:016x}", prefix, hash)
}

/// Check for legacy `output_N` / `input_N` IDs.
pub fn is_legacy_index_device_id(device_id: &str) -> bool {
    for prefix in ["output_", "input_"] {
        if let Some(rest) = device_id.strip_prefix(prefix) {
            return rest.parse::<usize>().is_ok();
        }
    }
    false
}

pub struct DeviceManager {
    host: Host,
}

impl DeviceManager {
    pub fn new() -> Self {
        Self {
            host: cpal::default_host(),
        }
    }

    /// Kept for the device-picker UI (planned in Phase 7.1) — nothing calls it yet.
    #[allow(dead_code)]
    pub fn get_output_devices(&self) -> Result<Vec<DeviceInfo>, String> {
        crate::always_print!("🔍 [DeviceManager] Starting audio output device enumeration...");
        let mut devices = Vec::new();
        let default_device = self.host.default_output_device();
        let default_name = default_device
            .as_ref()
            .and_then(|d| d.name().ok())
            .unwrap_or_else(|| "Unknown".to_string());

        crate::always_print!("🔍 [DeviceManager] Default device: {}", default_name);

        crate::always_print!("🔍 [DeviceManager] Enumerating output devices via ALSA/cpal...");
        match self.host.output_devices() {
            Ok(device_iter) => {
                for (index, device) in device_iter.enumerate() {
                    if let Ok(name) = device.name() {
                        #[cfg(target_os = "linux")]
                        {
                            if name.starts_with("hw:")
                                || name.starts_with("plughw:")
                                || name.starts_with("dmix:")
                                || name.starts_with("dsnoop:")
                                || name.starts_with("front:")
                                || name.starts_with("surround")
                                || name.starts_with("iec958:")
                            {
                                crate::always_print!("🔍 [DeviceManager] Skipping low-level ALSA alias: {}", name);
                                continue;
                            }
                        }

                        let is_default =
                            Some(&name) ==
                            default_device
                                .as_ref()
                                .and_then(|d| d.name().ok())
                                .as_ref();

                        crate::always_print!("🔍 [DeviceManager] Found device #{}: {} {}",
                            index,
                            name,
                            if is_default { "(default)" } else { "" }
                        );

                        devices.push(DeviceInfo {
                            id: device_id_from_name("output", &name),
                            name: name.clone(),
                            is_default,
                        });
                    }
                }
                crate::always_print!("✅ [DeviceManager] Enumeration complete. Found {} devices", devices.len());
            }
            Err(e) => {
                crate::always_print!("❌ [DeviceManager] Failed to enumerate: {}", e);
                return Err(format!("Failed to enumerate output devices: {}", e));
            }
        }

        if devices.is_empty() {
            crate::always_print!("⚠️ [DeviceManager] No devices found, adding default fallback");
            devices.push(DeviceInfo {
                id: "output_default".to_string(),
                name: default_name,
                is_default: true,
            });
        }

        crate::always_print!("🔍 [DeviceManager] Returning {} total devices", devices.len());
        Ok(devices)
    }

    pub fn get_output_device_by_id(&self, device_id: &str) -> Result<Option<Device>, String> {
        if device_id == "output_default" {
            return Ok(self.host.default_output_device());
        }

        match self.host.output_devices() {
            Ok(device_iter) => {
                // Legacy IDs resolve by position for migration.
                let legacy_index = device_id
                    .strip_prefix("output_")
                    .and_then(|rest| rest.parse::<usize>().ok());

                for (index, device) in device_iter.enumerate() {
                    if legacy_index == Some(index) {
                        return Ok(Some(device));
                    }
                    if let Ok(name) = device.name() {
                        if device_id_from_name("output", &name) == device_id {
                            return Ok(Some(device));
                        }
                    }
                }
            }
            Err(e) => {
                return Err(format!("Failed to enumerate devices: {}", e));
            }
        }

        Ok(None)
    }

    pub fn get_current_output_sample_rate(&self) -> Option<u32> {
        let config = crate::state::config_writer::current();

        let device = match &config.selected_audio_device {
            Some(device_id) =>
                match self.get_output_device_by_id(device_id) {
                    Ok(Some(device)) => Some(device),
                    _ => self.host.default_output_device(),
                }
            None => self.host.default_output_device(),
        };

        device.and_then(|d| d.default_output_config().ok()).map(|c| c.sample_rate().0)
    }
}

impl Default for DeviceManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_id_is_stable_for_a_given_name() {
        assert_eq!(
            device_id_from_name("output", "Speakers (Realtek High Definition Audio)"),
            device_id_from_name("output", "Speakers (Realtek High Definition Audio)")
        );
        assert_eq!(device_id_from_name("output", "Headphones"), "output_name:0b3a976597860826");
    }

    #[test]
    fn device_id_differs_by_name_and_by_direction() {
        assert_ne!(
            device_id_from_name("output", "Headphones"),
            device_id_from_name("output", "Speakers")
        );
        assert_ne!(
            device_id_from_name("output", "Headset"),
            device_id_from_name("input", "Headset")
        );
    }

    #[test]
    fn legacy_index_ids_are_recognised() {
        assert!(is_legacy_index_device_id("output_0"));
        assert!(is_legacy_index_device_id("input_12"));

        assert!(!is_legacy_index_device_id(&device_id_from_name("output", "Headphones")));
        assert!(!is_legacy_index_device_id("default"));
        assert!(!is_legacy_index_device_id("output_default"));
    }
}
