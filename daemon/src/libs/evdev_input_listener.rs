use std::thread;

#[cfg(target_os = "linux")]
use crossbeam_channel::Sender;

#[cfg(target_os = "linux")]
pub fn start_evdev_keyboard_listener(
    keyboard_tx: Sender<String>,
    hotkey_tx: Sender<String>,
) {
    crate::always_print!("🔍 [evdev] start_evdev_keyboard_listener() called - spawning thread");
    thread::spawn(move || {
        use evdev::KeyCode;

        crate::always_print!("🔍 [evdev] Thread started - initializing keyboard listener");
        crate::always_print!("🔍 [evdev] Current user: {:?}", std::env::var("USER"));
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
            return;
        }

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
                        let key = evdev::KeyCode(event.code());
                        let key_code = map_evdev_keycode(key);
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

#[cfg(target_os = "linux")]
fn map_evdev_keycode(key: evdev::KeyCode) -> &'static str {
    use evdev::KeyCode;

    match key {
        KeyCode::KEY_A => "KeyA", KeyCode::KEY_B => "KeyB", KeyCode::KEY_C => "KeyC", KeyCode::KEY_D => "KeyD",
        KeyCode::KEY_E => "KeyE", KeyCode::KEY_F => "KeyF", KeyCode::KEY_G => "KeyG", KeyCode::KEY_H => "KeyH",
        KeyCode::KEY_I => "KeyI", KeyCode::KEY_J => "KeyJ", KeyCode::KEY_K => "KeyK", KeyCode::KEY_L => "KeyL",
        KeyCode::KEY_M => "KeyM", KeyCode::KEY_N => "KeyN", KeyCode::KEY_O => "KeyO", KeyCode::KEY_P => "KeyP",
        KeyCode::KEY_Q => "KeyQ", KeyCode::KEY_R => "KeyR", KeyCode::KEY_S => "KeyS", KeyCode::KEY_T => "KeyT",
        KeyCode::KEY_U => "KeyU", KeyCode::KEY_V => "KeyV", KeyCode::KEY_W => "KeyW", KeyCode::KEY_X => "KeyX",
        KeyCode::KEY_Y => "KeyY", KeyCode::KEY_Z => "KeyZ",

        KeyCode::KEY_1 => "Digit1", KeyCode::KEY_2 => "Digit2", KeyCode::KEY_3 => "Digit3", KeyCode::KEY_4 => "Digit4",
        KeyCode::KEY_5 => "Digit5", KeyCode::KEY_6 => "Digit6", KeyCode::KEY_7 => "Digit7", KeyCode::KEY_8 => "Digit8",
        KeyCode::KEY_9 => "Digit9", KeyCode::KEY_0 => "Digit0",

        KeyCode::KEY_F1 => "F1", KeyCode::KEY_F2 => "F2", KeyCode::KEY_F3 => "F3", KeyCode::KEY_F4 => "F4",
        KeyCode::KEY_F5 => "F5", KeyCode::KEY_F6 => "F6", KeyCode::KEY_F7 => "F7", KeyCode::KEY_F8 => "F8",
        KeyCode::KEY_F9 => "F9", KeyCode::KEY_F10 => "F10", KeyCode::KEY_F11 => "F11", KeyCode::KEY_F12 => "F12",

        KeyCode::KEY_SPACE => "Space",
        KeyCode::KEY_ENTER => "Enter",
        KeyCode::KEY_BACKSPACE => "Backspace",
        KeyCode::KEY_TAB => "Tab",
        KeyCode::KEY_ESC => "Escape",
        KeyCode::KEY_CAPSLOCK => "CapsLock",
        KeyCode::KEY_LEFTSHIFT => "ShiftLeft",
        KeyCode::KEY_RIGHTSHIFT => "ShiftRight",
        KeyCode::KEY_LEFTCTRL => "ControlLeft",
        KeyCode::KEY_RIGHTCTRL => "ControlRight",
        KeyCode::KEY_LEFTALT => "AltLeft",
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

        KeyCode::KEY_MINUS => "Minus",
        KeyCode::KEY_EQUAL => "Equal",
        KeyCode::KEY_LEFTBRACE => "BracketLeft",
        KeyCode::KEY_RIGHTBRACE => "BracketRight",
        KeyCode::KEY_BACKSLASH => "Backslash",
        KeyCode::KEY_SEMICOLON => "Semicolon",
        KeyCode::KEY_APOSTROPHE => "Quote",
        KeyCode::KEY_GRAVE => "Backquote",
        KeyCode::KEY_COMMA => "Comma",
        KeyCode::KEY_DOT => "Period",
        KeyCode::KEY_SLASH => "Slash",
        
        _ => "",
    }
}
