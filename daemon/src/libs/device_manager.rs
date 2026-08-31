use cpal::traits::{ DeviceTrait, HostTrait };
use cpal::{ Device, Host };

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[cfg(target_os = "linux")]
struct AlsaErrorSuppressor {
    _stderr_fd: std::os::fd::OwnedFd,
}

#[cfg(target_os = "linux")]
impl AlsaErrorSuppressor {
    fn new() -> Self {
        use std::os::fd::{FromRawFd, OwnedFd};

        unsafe {
            let null_fd = libc::open(
                b"/dev/null\0".as_ptr() as *const libc::c_char,
                libc::O_WRONLY
            );
            let stderr_fd = libc::dup(libc::STDERR_FILENO);
            libc::dup2(null_fd, libc::STDERR_FILENO);
            libc::close(null_fd);

            Self {
                _stderr_fd: OwnedFd::from_raw_fd(stderr_fd),
            }
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for AlsaErrorSuppressor {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;

        unsafe {
            libc::dup2(self._stderr_fd.as_raw_fd(), libc::STDERR_FILENO);
        }
    }
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

impl Clone for DeviceManager {
    fn clone(&self) -> Self {
        Self {
            host: cpal::default_host(),
        }
    }
}

impl DeviceManager {
    pub fn new() -> Self {
        Self {
            host: cpal::default_host(),
        }
    }

    pub fn get_output_devices(&self) -> Result<Vec<DeviceInfo>, String> {
        crate::always_print!("🔍 [DeviceManager] Starting audio output device enumeration...");
        let mut devices = Vec::new();
        let default_device = self.host.default_output_device();
        let default_name = default_device
            .as_ref()
            .and_then(|d| d.name().ok())
            .unwrap_or_else(|| "Unknown".to_string());

        crate::always_print!("🔍 [DeviceManager] Default device: {}", default_name);

        #[cfg(target_os = "linux")]
        let _alsa_suppressor = AlsaErrorSuppressor::new();

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

    pub fn get_input_devices(&self) -> Result<Vec<DeviceInfo>, String> {
        let mut devices = Vec::new();
        let default_device = self.host.default_input_device();
        let default_name = default_device
            .as_ref()
            .and_then(|d| d.name().ok())
            .unwrap_or_else(|| "Unknown".to_string());

        #[cfg(target_os = "linux")]
        let _alsa_suppressor = AlsaErrorSuppressor::new();

        match self.host.input_devices() {
            Ok(device_iter) => {
                for device in device_iter {
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
                                continue;
                            }
                        }

                        let is_default =
                            Some(&name) ==
                            default_device
                                .as_ref()
                                .and_then(|d| d.name().ok())
                                .as_ref();

                        devices.push(DeviceInfo {
                            id: device_id_from_name("input", &name),
                            name: name.clone(),
                            is_default,
                        });
                    }
                }
            }
            Err(e) => {
                return Err(format!("Failed to enumerate input devices: {}", e));
            }
        }

        if devices.is_empty() {
            devices.push(DeviceInfo {
                id: "input_default".to_string(),
                name: default_name,
                is_default: true,
            });
        }

        Ok(devices)
    }

    pub fn get_output_device_by_id(&self, device_id: &str) -> Result<Option<Device>, String> {
        if device_id == "output_default" {
            return Ok(self.host.default_output_device());
        }

        #[cfg(target_os = "linux")]
        let _alsa_suppressor = AlsaErrorSuppressor::new();

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

    pub fn get_input_device_by_id(&self, device_id: &str) -> Result<Option<Device>, String> {
        if device_id == "input_default" {
            return Ok(self.host.default_input_device());
        }

        #[cfg(target_os = "linux")]
        let _alsa_suppressor = AlsaErrorSuppressor::new();

        match self.host.input_devices() {
            Ok(device_iter) => {
                let legacy_index = device_id
                    .strip_prefix("input_")
                    .and_then(|rest| rest.parse::<usize>().ok());

                for (index, device) in device_iter.enumerate() {
                    if legacy_index == Some(index) {
                        return Ok(Some(device));
                    }
                    if let Ok(name) = device.name() {
                        if device_id_from_name("input", &name) == device_id {
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

    pub fn test_output_device(&self, device_id: &str) -> Result<bool, String> {
        #[cfg(target_os = "linux")]
        let _alsa_suppressor = AlsaErrorSuppressor::new();

        match self.get_output_device_by_id(device_id)? {
            Some(device) => {
                match device.supported_output_configs() {
                    Ok(mut configs) => Ok(configs.next().is_some()),
                    Err(_) => Ok(false),
                }
            }
            None => Ok(false),
        }
    }

    pub fn test_input_device(&self, device_id: &str) -> Result<bool, String> {
        #[cfg(target_os = "linux")]
        let _alsa_suppressor = AlsaErrorSuppressor::new();

        match self.get_input_device_by_id(device_id)? {
            Some(device) => {
                match device.supported_input_configs() {
                    Ok(mut configs) => Ok(configs.next().is_some()),
                    Err(_) => Ok(false),
                }
            }
            None => Ok(false),
        }
    }

    /// Sample rate of selected output device, if available.
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

    /// Check if device natively supports a sample rate.
    pub fn device_supports_rate(&self, device_id: Option<&str>, rate: u32) -> bool {
        let device = match device_id {
            Some(id) => match self.get_output_device_by_id(id) {
                Ok(Some(d)) => Some(d),
                _ => self.host.default_output_device(),
            },
            None => self.host.default_output_device(),
        };
        if let Some(dev) = device {
            if let Ok(mut configs) = dev.supported_output_configs() {
                return configs.any(|c| c.min_sample_rate().0 <= rate && rate <= c.max_sample_rate().0);
            }
        }
        false
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
