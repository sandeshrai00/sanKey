//! Runtime health shared between input listener, engine, and control API.
//! Explains "daemon running but silent": input permission, pack load, audio.
//! All atomics/locks — safe to call from any thread, never blocks the hot path.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

static KEYBOARDS: AtomicUsize = AtomicUsize::new(0);
static LAST_KEY_UNIX: AtomicU64 = AtomicU64::new(0);
static PACK_STATE: AtomicU8 = AtomicU8::new(0); // 0=unknown, 1=loaded, 2=failed
static AUDIO_OK: AtomicBool = AtomicBool::new(false);

fn input_error_slot() -> &'static Mutex<Option<String>> {
    static SLOT: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn pack_error_slot() -> &'static Mutex<Option<String>> {
    static SLOT: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn audio_error_slot() -> &'static Mutex<Option<String>> {
    static SLOT: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Input listener reports how many keyboards it holds.
pub fn set_input_keyboards(n: usize) {
    KEYBOARDS.store(n, Ordering::Relaxed);
}

/// Input listener reports a problem (e.g. no /dev/input access).
/// `None` clears the error.
pub fn set_input_error(err: Option<String>) {
    if let Ok(mut slot) = input_error_slot().lock() {
        *slot = err;
    }
}

/// Record that a key event reached the engine (call on keydown).
pub fn note_key() {
    LAST_KEY_UNIX.store(now_unix(), Ordering::Relaxed);
}

/// Engine reports pack load outcome.
pub fn set_pack_result(loaded: bool, err: Option<String>) {
    PACK_STATE.store(if loaded { 1 } else { 2 }, Ordering::Relaxed);
    if let Ok(mut slot) = pack_error_slot().lock() {
        *slot = err;
    }
}

/// Engine reports audio backend health.
pub fn set_audio_result(ok: bool, err: Option<String>) {
    AUDIO_OK.store(ok, Ordering::Relaxed);
    if let Ok(mut slot) = audio_error_slot().lock() {
        *slot = err;
    }
}

fn get_opt(slot: &'static Mutex<Option<String>>) -> Option<String> {
    slot.lock().ok().and_then(|g| g.clone())
}

/// Snapshot merged into `status` and `diag` responses.
pub fn snapshot() -> serde_json::Value {
    let now = now_unix();
    let last = LAST_KEY_UNIX.load(Ordering::Relaxed);
    serde_json::json!({
        "input_keyboards": KEYBOARDS.load(Ordering::Relaxed),
        "input_error": get_opt(input_error_slot()),
        "last_key_age_s": if last == 0 { serde_json::Value::Null } else { serde_json::json!(now.saturating_sub(last)) },
        "pack_loaded": match PACK_STATE.load(Ordering::Relaxed) {
            1 => serde_json::json!(true),
            2 => serde_json::json!(false),
            _ => serde_json::Value::Null,
        },
        "pack_error": get_opt(pack_error_slot()),
        "audio_ok": AUDIO_OK.load(Ordering::Relaxed),
        "audio_error": get_opt(audio_error_slot()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_snapshot_reports_what_was_set() {
        set_input_keyboards(2);
        set_input_error(Some("test error".to_string()));
        set_pack_result(false, Some("pack boom".to_string()));
        set_audio_result(true, None);
        let s = snapshot();
        assert_eq!(s["input_keyboards"], 2);
        assert_eq!(s["input_error"], "test error");
        assert_eq!(s["pack_loaded"], false);
        assert_eq!(s["pack_error"], "pack boom");
        assert_eq!(s["audio_ok"], true);
        // reset for other tests
        set_input_keyboards(0);
        set_input_error(None);
        PACK_STATE.store(0, Ordering::Relaxed);
        set_pack_result(true, None);
    }
}
