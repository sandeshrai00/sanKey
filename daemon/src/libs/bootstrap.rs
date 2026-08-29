use crossbeam_channel::Sender;

pub fn start_input_capture(keyboard_tx: Sender<String>, hotkey_tx: Sender<String>) {
    #[cfg(target_os = "linux")]
    {
        crate::debug_print!("🎮 Starting evdev keyboard listener...");
        crate::libs::evdev_input_listener::start_evdev_keyboard_listener(
            keyboard_tx.clone(),
            hotkey_tx.clone(),
        );
        crate::debug_print!("🎮 Starting unified input listener (fallback)...");
        crate::libs::input_listener::start_unified_input_listener(keyboard_tx, hotkey_tx);
    }
}
