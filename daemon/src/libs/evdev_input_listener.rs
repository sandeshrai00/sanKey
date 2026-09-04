use std::thread;

#[cfg(target_os = "linux")]
use crossbeam_channel::Sender;
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(target_os = "linux")]
use std::sync::{Mutex, OnceLock};

/// Snapshot of keyboard-capture health. Surfaced via `ctl status/diag`
/// so the panel can show "No keyboard access" instead of "Playing".
#[cfg(target_os = "linux")]
#[derive(Debug, Clone)]
pub struct InputHealth {
    pub total: usize,
    pub keyboards: usize,
    pub initialized: usize,
    pub ok: bool,
    pub hint: String,
}

#[cfg(target_os = "linux")]
static HEALTH: OnceLock<Mutex<InputHealth>> = OnceLock::new();
#[cfg(target_os = "linux")]
static INITIALIZED: AtomicUsize = AtomicUsize::new(0);

#[cfg(target_os = "linux")]
fn health_slot() -> &'static Mutex<InputHealth> {
    HEALTH.get_or_init(|| {
        Mutex::new(InputHealth {
            total: 0,
            keyboards: 0,
            initialized: 0,
            ok: false,
            hint: String::new(),
        })
    })
}

#[cfg(target_os = "linux")]
fn set_health(total: usize, keyboards: usize, hint: String) {
    let initialized = INITIALIZED.load(Ordering::Relaxed);
    let ok = initialized > 0;
    if let Ok(mut guard) = health_slot().lock() {
        *guard = InputHealth {
            total,
            keyboards,
            initialized,
            ok,
            hint,
        };
    }
}

/// Current snapshot (cheap, no enumeration).
#[cfg(target_os = "linux")]
pub fn health() -> InputHealth {
    health_slot()
        .lock()
        .map(|g| g.clone())
        .unwrap_or(InputHealth {
            total: 0,
            keyboards: 0,
            initialized: 0,
            ok: false,
            hint: "health unavailable".to_string(),
        })
}

/// On-demand re-probe (panel Rescan). Enumerates but never spawns
/// threads: if keyboards appear after a group fix, the daemon still
/// needs a restart (group applies at login), so hint says so.
#[cfg(target_os = "linux")]
pub fn probe_input() -> InputHealth {
    let devices: Vec<_> = evdev::enumerate().collect();
    let total = devices.len();
    let mut keyboards = 0usize;
    if total > 0 {
        use evdev::KeyCode;
        for (_, device) in &devices {
            if device
                .supported_keys()
                .is_some_and(|k| k.contains(KeyCode::KEY_A))
            {
                keyboards += 1;
            }
        }
    }
    let initialized = INITIALIZED.load(Ordering::Relaxed);
    let hint = if initialized > 0 {
        String::new()
    } else if total == 0 {
        "cannot open /dev/input/event* — join the 'input' group, relogin, restart sorakey".to_string()
    } else if keyboards == 0 {
        "no keyboard among input devices — permission or hardware issue".to_string()
    } else {
        "keyboards visible — restart sorakey to start capture".to_string()
    };
    if let Ok(mut guard) = health_slot().lock() {
        *guard = InputHealth {
            total,
            keyboards,
            initialized,
            ok: initialized > 0,
            hint: hint.clone(),
        };
        return guard.clone();
    }
    InputHealth {
        total,
        keyboards,
        initialized,
        ok: initialized > 0,
        hint,
    }
}

#[cfg(target_os = "linux")]
pub fn start_evdev_keyboard_listener(
    keyboard_tx: Sender<String>,
    hotkey_tx: Sender<String>,
) {
    // Keeper: the engine's `select!` spins at 100% CPU if every sender
    // disconnects (early return below would drop both). Park clones forever
    // so the receivers stay connected with zero traffic when blocked.
    {
        let kb = keyboard_tx.clone();
        let hk = hotkey_tx.clone();
        thread::spawn(move || {
            let _keep = (kb, hk);
            std::thread::park();
        });
    }
    crate::always_print!("🔍 [evdev] start_evdev_keyboard_listener() called - spawning thread");
    thread::spawn(move || {
        use evdev::KeyCode;

        crate::always_print!("🔍 [evdev] Thread started - initializing keyboard listener");
        crate::always_print!("🔍 [evdev] Starting Linux keyboard listener (Wayland/X11 compatible)");

        let mut keyboards = Vec::new();

        crate::always_print!("🔍 [evdev] Enumerating input devices...");
        let devices: Vec<_> = evdev::enumerate().collect();
        let device_count = devices.len();
        crate::always_print!("🔍 [evdev] Found {} total input devices", device_count);

        if device_count == 0 {
            crate::always_eprint!("❌ [evdev] No devices found - cannot access /dev/input/event* devices");
            crate::always_eprint!("💡 [evdev] Troubleshooting steps:");
            crate::always_eprint!("   1. Check if you're in the 'input' group: groups $USER");
            crate::always_eprint!("   2. Add yourself to input group: sudo usermod -a -G input $USER");
            crate::always_eprint!("   3. Log out and log back in for group changes to take effect");
            crate::always_eprint!("   4. Check /dev/input permissions: ls -la /dev/input/event*");
            set_health(
                device_count,
                0,
                "cannot open /dev/input/event* — join the 'input' group, relogin, restart sorakey".to_string(),
            );
            return;
        }

        for (path, device) in devices {
            let is_keyboard = device.supported_keys().is_some_and(|k| k.contains(KeyCode::KEY_A));
            if is_keyboard {
                crate::always_print!("🔍 [evdev] Found keyboard device: {:?} - {}", path.display(), device.name().unwrap_or("Unknown"));
                keyboards.push((path, device));
            } else {
                crate::always_print!("🔍 [evdev] Skipping non-keyboard device: {:?}", path.display());
            }
        }

        if keyboards.is_empty() {
            crate::always_eprint!("❌ [evdev] No keyboard devices found among the {} input devices!", device_count);
            crate::always_eprint!("💡 [evdev] This might indicate a permission issue or unusual hardware setup");
            set_health(
                device_count,
                0,
                "no keyboard among input devices — permission or hardware issue".to_string(),
            );
            return;
        }

        INITIALIZED.store(keyboards.len(), Ordering::Relaxed);
        set_health(device_count, keyboards.len(), String::new());
        crate::always_print!("✅ [evdev] Successfully initialized {} keyboard device(s)", keyboards.len());
        crate::always_print!("🔍 [evdev] Starting blocking event threads...");

        // One blocking thread per keyboard: fetch_events blocks until the
        // kernel delivers an event, so key-to-channel latency is ~0 instead
        // of the old 20ms poll interval. Unplugged devices error out and the
        // thread just exits (the kernel releases the fd).
        for (path, mut device) in keyboards {
            let keyboard_tx = keyboard_tx.clone();
            let hotkey_tx = hotkey_tx.clone();
            thread::spawn(move || {
                use evdev::EventType;

                let mut ctrl_pressed = false;
                let mut alt_pressed = false;
                let mut event_count = 0;
                let mut first_event_logged = false;

                loop {
                    let mut events = match device.fetch_events() {
                        Ok(e) => e,
                        Err(e) => {
                            crate::always_eprint!("⚠️ [evdev] Error on {}: {} - stopping thread", path.display(), e);
                            break;
                        }
                    };
                    for event in events.by_ref() {
                        if event.event_type() != EventType::KEY {
                            continue;
                        }
                        event_count += 1;
                        if !first_event_logged {
                            crate::always_print!("✅ [evdev] First keyboard event detected!");
                            first_event_logged = true;
                        }

                        let key_value = event.value();
                        let key_code = map_evdev_keycode(event.code());
                        if key_code.is_empty() {
                            continue;
                        }

                        if key_value == 1 {
                            match key_code {
                                "ControlLeft" | "ControlRight" => ctrl_pressed = true,
                                "AltLeft" | "AltRight" => alt_pressed = true,
                                "KeyM" => {
                                    if ctrl_pressed && alt_pressed {
                                        crate::always_print!("🔥 [evdev] Hotkey detected: Ctrl+Alt+M - Toggling global sound");
                                        let _ = hotkey_tx.send("TOGGLE_SOUND".to_string());
                                        continue;
                                    }
                                }
                                _ => {}
                            }

                            if event_count <= 5 {
                                crate::always_print!("🔍 [evdev] Sending key press: {}", key_code);
                            }
                            let _ = keyboard_tx.send(key_code.to_string());
                        } else if key_value == 0 {
                            match key_code {
                                "ControlLeft" | "ControlRight" => ctrl_pressed = false,
                                "AltLeft" | "AltRight" => alt_pressed = false,
                                _ => {}
                            }

                            let _ = keyboard_tx.send(format!("UP:{}", key_code));
                        }
                    }
                }
            });
        }
    });
}

/// W3C name for a raw evdev key code; "" for keys the daemon does not sound.
/// The standard range (1-88) comes from the shared table so runtime key names
/// cannot drift from what the V1→V2 converter emits. Above 88 the evdev and
/// IOHook keyspaces diverge, so the evdev-specific extended codes live here.
#[cfg(target_os = "linux")]
fn map_evdev_keycode(code: u16) -> &'static str {
    if let Some(name) = crate::utils::keymap::w3c_name(code) {
        return name;
    }

    use evdev::KeyCode;
    match KeyCode(code) {
        KeyCode::KEY_RIGHTCTRL => "ControlRight",
        KeyCode::KEY_RIGHTALT => "AltRight",
        KeyCode::KEY_LEFTMETA => "MetaLeft",
        KeyCode::KEY_RIGHTMETA => "MetaRight",

        KeyCode::KEY_UP => "ArrowUp",
        KeyCode::KEY_DOWN => "ArrowDown",
        KeyCode::KEY_LEFT => "ArrowLeft",
        KeyCode::KEY_RIGHT => "ArrowRight",

        KeyCode::KEY_INSERT => "Insert",
        KeyCode::KEY_DELETE => "Delete",
        KeyCode::KEY_HOME => "Home",
        KeyCode::KEY_END => "End",
        KeyCode::KEY_PAGEUP => "PageUp",
        KeyCode::KEY_PAGEDOWN => "PageDown",

        _ => "",
    }
}
