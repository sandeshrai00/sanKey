use crossbeam_channel::Sender;
use rdev::{ listen, Event, EventType, Key };
use std::collections::HashSet;
use std::sync::{ Arc, Mutex };
use std::thread;
use std::time::{ Duration, Instant };

// Maps a keyboard key to its standardized code
fn map_key_to_code(key: Key) -> &'static str {
    match key {
        // Common keys across all platforms
        Key::Space => "Space",
        Key::Backspace => "Backspace",
        Key::CapsLock => "CapsLock",
        Key::Tab => "Tab",
        Key::Return => "Enter",
        Key::Escape => "Escape",
        Key::Delete => "Delete",

        // Modifier keys with left/right variants
        Key::Alt => "AltLeft",
        Key::AltGr => "AltRight",
        Key::ShiftLeft => "ShiftLeft",
        Key::ShiftRight => "ShiftRight",
        Key::ControlLeft => "ControlLeft",
        Key::ControlRight => "ControlRight",
        Key::MetaLeft => "MetaLeft",
        Key::MetaRight => "MetaRight",

        // Arrow keys
        Key::UpArrow => "ArrowUp",
        Key::DownArrow => "ArrowDown",
        Key::LeftArrow => "ArrowLeft",
        Key::RightArrow => "ArrowRight",

        // Navigation keys
        Key::Home => "Home",
        Key::End => "End",
        Key::PageUp => "PageUp",
        Key::PageDown => "PageDown",
        Key::Insert => "Insert", // Function keys F1-F12 (rdev 0.5.3 only supports F1-F12)
        Key::F1 => "F1",
        Key::F2 => "F2",
        Key::F3 => "F3",
        Key::F4 => "F4",
        Key::F5 => "F5",
        Key::F6 => "F6",
        Key::F7 => "F7",
        Key::F8 => "F8",
        Key::F9 => "F9",
        Key::F10 => "F10",
        Key::F11 => "F11",
        Key::F12 => "F12",

        // Alpha keys A-Z
        Key::KeyA => "KeyA",
        Key::KeyB => "KeyB",
        Key::KeyC => "KeyC",
        Key::KeyD => "KeyD",
        Key::KeyE => "KeyE",
        Key::KeyF => "KeyF",
        Key::KeyG => "KeyG",
        Key::KeyH => "KeyH",
        Key::KeyI => "KeyI",
        Key::KeyJ => "KeyJ",
        Key::KeyK => "KeyK",
        Key::KeyL => "KeyL",
        Key::KeyM => "KeyM",
        Key::KeyN => "KeyN",
        Key::KeyO => "KeyO",
        Key::KeyP => "KeyP",
        Key::KeyQ => "KeyQ",
        Key::KeyR => "KeyR",
        Key::KeyS => "KeyS",
        Key::KeyT => "KeyT",
        Key::KeyU => "KeyU",
        Key::KeyV => "KeyV",
        Key::KeyW => "KeyW",
        Key::KeyX => "KeyX",
        Key::KeyY => "KeyY",
        Key::KeyZ => "KeyZ",

        // Number keys 0-9
        Key::Num0 => "Digit0",
        Key::Num1 => "Digit1",
        Key::Num2 => "Digit2",
        Key::Num3 => "Digit3",
        Key::Num4 => "Digit4",
        Key::Num5 => "Digit5",
        Key::Num6 => "Digit6",
        Key::Num7 => "Digit7",
        Key::Num8 => "Digit8",
        Key::Num9 => "Digit9",

        // Punctuation and symbols
        Key::Minus => "Minus", // -
        Key::Equal => "Equal", // =
        Key::Comma => "Comma", // ,
        Key::Dot => "Period", // .
        Key::Quote => "Quote", // '
        Key::BackQuote => "Backquote", // `
        Key::Slash => "Slash", // /
        Key::LeftBracket => "BracketLeft", // [
        Key::RightBracket => "BracketRight", // ]
        Key::BackSlash => "Backslash", // \
        Key::SemiColon => "Semicolon", // ;
        Key::IntlBackslash => "IntlBackslash", // Additional backslash key on some keyboards

        // Numpad keys
        Key::KpReturn => "NumpadEnter",
        Key::KpMinus => "NumpadSubtract",
        Key::KpPlus => "NumpadAdd",
        Key::KpMultiply => "NumpadMultiply",
        Key::KpDivide => "NumpadDivide",
        Key::Kp0 => "Numpad0",
        Key::Kp1 => "Numpad1",
        Key::Kp2 => "Numpad2",
        Key::Kp3 => "Numpad3",
        Key::Kp4 => "Numpad4",
        Key::Kp5 => "Numpad5",
        Key::Kp6 => "Numpad6",
        Key::Kp7 => "Numpad7",
        Key::Kp8 => "Numpad8",
        Key::Kp9 => "Numpad9",
        Key::KpDelete => "NumpadDecimal",

        // Additional system keys
        Key::NumLock => "NumLock",
        Key::ScrollLock => "ScrollLock",
        Key::PrintScreen => "PrintScreen",
        Key::Pause => "Pause",
        Key::Function => "Fn", // Special function key on some keyboards

        // Unknown or unmapped keys
        Key::Unknown(_) => "", // Handle unknown keys gracefully
    }
}

/// Start a unified input listener that handles keyboard events
///
/// to avoid duplicate events with the focused_input_listener
pub fn start_unified_input_listener(
    keyboard_tx: Sender<String>,
    hotkey_tx: Sender<String>,
) {
    crate::always_print!("🎮 Starting unified input listener (keyboard + hotkeys)...");

    thread::spawn(move || {
        crate::always_print!("🎮 Unified input listener thread started");

        let keyboard_last_press = Arc::new(Mutex::new(Instant::now()));
        let pressed_keys = Arc::new(Mutex::new(HashSet::<String>::new()));

        // Track pressed modifier keys for hotkey detection
        let mut ctrl_pressed = false;
        let mut alt_pressed = false;

        crate::always_print!("🎮 Starting rdev::listen() - listening to keyboard events");
        let result = listen(move |event: Event| {
            match event.event_type {
                // ===== KEYBOARD EVENTS =====
                EventType::KeyPress(key) => {
                    let key_code = map_key_to_code(key);
                    if !key_code.is_empty() {
                        // Track modifier keys for hotkey detection
                        match key_code {
                            "ControlLeft" | "ControlRight" => {
                                ctrl_pressed = true;
                            }
                            "AltLeft" | "AltRight" => {
                                alt_pressed = true;
                            }
                            "KeyM" => {
                                // Check for Ctrl+Alt+M hotkey combination
                                if ctrl_pressed && alt_pressed {
                                    crate::always_print!(
                                        "🔥 Hotkey detected: Ctrl+Alt+M - Toggling global sound"
                                    );
                                    let _ = hotkey_tx.send("TOGGLE_SOUND".to_string());
                                    return; // Don't process this as a regular key event
                                }
                            }
                            _ => {}
                        }

                        // Check if key is already pressed
                        let mut pressed = pressed_keys.lock().unwrap();
                        if pressed.contains(&key_code.to_string()) {
                            return; // Key already pressed, ignore
                        }
                        pressed.insert(key_code.to_string());
                        drop(pressed); // Apply debounce and detect rapid key events
                        let now = Instant::now();
                        let mut last = keyboard_last_press.lock().unwrap();
                        let time_since_last = now.duration_since(*last);

                        // Special handling for Backspace key - skip if too rapid (< 10ms)
                        if key_code == "Backspace" && time_since_last < Duration::from_millis(10) {
                            return; // Skip this Backspace event entirely
                        }

                        if time_since_last > Duration::from_millis(1) {
                            *last = now;
                            let _ = keyboard_tx.send(key_code.to_string());
                        }
                    }
                }
                EventType::KeyRelease(key) => {
                    let key_code = map_key_to_code(key);
                    if !key_code.is_empty() {
                        // Track modifier key releases for hotkey detection
                        match key_code {
                            "ControlLeft" | "ControlRight" => {
                                ctrl_pressed = false;
                            }
                            "AltLeft" | "AltRight" => {
                                alt_pressed = false;
                            }
                            _ => {}
                        }

                        // Remove key from pressed set
                        let mut pressed = pressed_keys.lock().unwrap();
                        pressed.remove(&key_code.to_string());
                        drop(pressed);

                        let _ = keyboard_tx.send(format!("UP:{}", key_code));
                    }
                }

                // Ignore non-keyboard events
                EventType::ButtonPress(_) | EventType::ButtonRelease(_) | EventType::Wheel { .. } | EventType::MouseMove { .. } => {}
            }
        });

        if let Err(error) = result {
            crate::always_eprint!("❌ Unified input listener error: {:?}", error);
        }
    });
}