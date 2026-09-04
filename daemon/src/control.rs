//! Control API over Unix socket (`$XDG_RUNTIME_DIR/sorakey.sock`).
//! One JSON line in, one out. All writes go through config_writer + engine.

use crate::libs::audio::{ AudioCommand, AudioEngineHandle };
use crate::libs::cli_args::qualify_soundpack_id;
use crate::state::paths;
use std::io::{ BufRead, BufReader, Read, Write };
use std::os::unix::net::{ UnixListener, UnixStream };
use std::path::{ Path, PathBuf };

pub fn socket_path() -> PathBuf {
    match std::env::var("XDG_RUNTIME_DIR") {
        Ok(dir) => PathBuf::from(dir).join("sorakey.sock"),
        Err(_) => PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".sorakey.sock"),
    }
}

/// Spawn the accept loop. Returns the bound socket path.
pub fn serve(engine: AudioEngineHandle) -> Option<PathBuf> {
    let path = socket_path();
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            crate::always_eprint!("❌ [control] cannot bind {}: {}", path.display(), e);
            return None;
        }
    };
    let _ = std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600));

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let eng = engine.clone();
                    let _ = std::thread::Builder::new()
                        .stack_size(64 * 1024)
                        .spawn(move || {
                            let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(100)));
                            handle_conn(stream, &eng)
                        });
                }
                Err(e) => crate::always_print!("⚠️  [control] accept: {}", e),
            }
        }
    });

    Some(path)
}

fn handle_conn(mut stream: UnixStream, engine: &AudioEngineHandle) {
    const MAX_REQUEST_BYTES: u64 = 64 * 1024;
    let mut buf: Vec<u8> = Vec::with_capacity(256);
    let mut chunk = [0u8; 4096];
    let mut reader = BufReader::new(&stream);
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if (buf.len() as u64 + n as u64) > MAX_REQUEST_BYTES {
                    let _ = stream.write_all(b"{\"ok\":false,\"error\":\"request too large\"}\n");
                    return;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            Err(_) => return,
        }
        if buf.contains(&b'\n') {
            break;
        }
    }
    let Some(newline) = buf.iter().position(|b| *b == b'\n') else {
        return;
    };
    let line = String::from_utf8_lossy(&buf[..newline]).into_owned();
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    let response = dispatch(line, engine);
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(b"\n");
}

fn dispatch(request: &str, engine: &AudioEngineHandle) -> String {
    let req: serde_json::Value = match serde_json::from_str(request) {
        Ok(v) => v,
        Err(e) => return fail(&format!("bad json: {e}")),
    };
    let cmd = req.get("cmd").and_then(|c| c.as_str()).unwrap_or("");

    match cmd {
        "status" => status(),
        "set_bar_section" => set_bar_section(&req),
        "get_bar_section" => get_bar_section(),
        "mute" => {
            let muted = req.get("muted").and_then(|v| v.as_bool()).unwrap_or(true);
            crate::state::config_writer::apply(|c| c.enable_sound = !muted);
            engine.send(AudioCommand::SetSoundEnabled(!muted));
            ok(serde_json::json!({ "muted": muted }))
        }
        "volume" => {
            let v = match clamp_percent(req.get("value")) { Some(v) => v, None => return fail("value must be 0-100") };
            let f = v / 100.0;
            let cur = crate::state::config_writer::current();
            if !cur.keyboard_soundpack.is_empty() {
                let id = cur.keyboard_soundpack.clone();
                if too_many_per_pack_entries(&id) {
                    return fail("too many per-pack entries");
                }
                crate::state::config_writer::apply(|c| { c.per_pack_volume.insert(id, f); });
            } else {
                crate::state::config_writer::apply(|c| c.volume = f);
            }
            let eff = crate::state::config_writer::current().effective_volume();
            engine.send(AudioCommand::SetVolume(eff));
            ok(serde_json::json!({ "volume": v }))
        }
        "per_pack_volume" => per_pack_volume(&req, engine),
        "reset_volume" => reset_volume(&req, engine),
        "delete_pack" => delete_pack(&req, engine),
        "keyboard_pack" => load_pack(&req, engine),
        "packs" => packs(),
        "audio_devices" => audio_devices(),
        "select_device" => select_device(&req, engine),
        "diag" => diag(),
        "export_logs" => export_logs(),
        "key" => key_event(&req, engine),
        "toggle_mute" => toggle_mute(engine),
        other => fail(&format!("unknown cmd: {other}")),
    }
}

fn clamp_percent(v: Option<&serde_json::Value>) -> Option<f32> {
    let n = v?.as_f64()?;
    if !(0.0..=100.0).contains(&n) {
        return None;
    }
    Some(n as f32)
}

fn status() -> String {
    let c = crate::state::config_writer::current();
    let eff = c.effective_volume();
    let per = c.per_pack_volume.get(&c.keyboard_soundpack).copied().unwrap_or(eff);
    let mut v = serde_json::json!({
        "running": true,
        "muted": !c.enable_sound,
        "volume": (eff * 100.0).round(),
        "per_pack_volume": (per * 100.0).round(),
        "keyboard_pack": c.keyboard_soundpack,
        "audio_device": c.selected_audio_device,
    });
    // Health explains "running but silent" (input capture, pack load, audio).
    if let Some(obj) = v.as_object_mut() {
        for (k, val) in crate::state::health::snapshot().as_object().cloned().unwrap_or_default() {
            obj.insert(k, val);
        }
    }
    ok(v)
}

/// Fast key ingest for notifiers that only hold an engine handle.
fn key_event(req: &serde_json::Value, engine: &AudioEngineHandle) -> String {
    let code = match req.get("code").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return fail("missing code"),
    };
    if code.len() > 32
        || !code.bytes().all(|b| b.is_ascii_alphanumeric())
        || !is_known_key_code(code)
    {
        return fail("unknown code");
    }
    let down = req.get("down").and_then(|v| v.as_bool()).unwrap_or(true);
    engine.send(AudioCommand::Key { code: code.to_string(), down });
    ok(serde_json::json!({ "code": code, "down": down }))
}

fn is_known_key_code(code: &str) -> bool {
    if crate::utils::keymap::KEY_MAP.iter().any(|&(_, n)| n == code) {
        return true;
    }
    matches!(
        code,
        "ControlRight"
            | "AltRight"
            | "MetaLeft"
            | "MetaRight"
            | "ArrowUp"
            | "ArrowDown"
            | "ArrowLeft"
            | "ArrowRight"
            | "Insert"
            | "Delete"
            | "Home"
            | "End"
            | "PageUp"
            | "PageDown"
    )
}

fn toggle_mute(engine: &AudioEngineHandle) -> String {
    let mut enabled = false;
    crate::state::config_writer::apply(|config| {
        config.enable_sound = !config.enable_sound;
        enabled = config.enable_sound;
    });
    engine.send(AudioCommand::SetSoundEnabled(enabled));
    crate::always_print!("🔄 [control] Sound toggled: {}", enabled);
    ok(serde_json::json!({ "muted": !enabled }))
}

fn recommended_volume_for(id: &str) -> Option<f32> {
    let path = paths::soundpacks::config_json(id);
    let content = std::fs::read_to_string(&path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&content).ok()?;
    v.get("options")?.get("recommended_volume")?.as_f64().map(|n| n as f32)
}

fn load_pack(req: &serde_json::Value, engine: &AudioEngineHandle) -> String {
    let raw = match req.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return fail("missing id"),
    };
    if raw.contains('\0') || raw.contains("..") {
        return fail("invalid id");
    }
    let id = qualify_soundpack_id(&raw, "keyboard/");
    let rec = recommended_volume_for(&id);
    let will_insert = rec.is_some_and(|v| {
        let v = v.clamp(0.1, 1.0);
        (v - 1.0).abs() > 0.001
    }) && !crate::state::config_writer::current().per_pack_volume.contains_key(&id);
    if will_insert && too_many_per_pack_entries(&id) {
        return fail("too many per-pack entries");
    }

    crate::state::config_writer::apply(|c| {
        c.keyboard_soundpack = id.clone();
        if !c.per_pack_volume.contains_key(&id) {
            if let Some(v) = rec {
                let v = v.clamp(0.1, 1.0);
                if (v - 1.0).abs() > 0.001 {
                    c.per_pack_volume.insert(id.clone(), v);
                }
            }
        }
    });
    let eff = crate::state::config_writer::current().effective_volume();
    engine.send(AudioCommand::LoadKeyboardPack { soundpack_id: id.clone(), update_cache_on_error: true });
    engine.send(AudioCommand::SetVolume(eff));
    ok(serde_json::json!({ "id": id }))
}

fn reset_volume(req: &serde_json::Value, engine: &AudioEngineHandle) -> String {
    let raw = match req.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return fail("missing id"),
    };
    if raw.contains('\0') || raw.contains("..") {
        return fail("invalid id");
    }
    let id = qualify_soundpack_id(&raw, "keyboard/");
    crate::state::config_writer::apply(|c| {
        c.per_pack_volume.remove(&id);
        if let Some(v) = recommended_volume_for(&id) {
            let v = v.clamp(0.1, 1.0);
            if (v - 1.0).abs() > 0.001 {
                c.per_pack_volume.insert(id.clone(), v);
            }
        }
    });
    let cur = crate::state::config_writer::current();
    if cur.keyboard_soundpack == id {
        engine.send(AudioCommand::SetVolume(cur.effective_volume()));
    }
    let per = cur.per_pack_volume.get(&id).copied().unwrap_or(1.0) * 100.0;
    ok(serde_json::json!({ "id": id, "per_pack_volume": per.round() }))
}

fn per_pack_volume(req: &serde_json::Value, engine: &AudioEngineHandle) -> String {
    let raw = match req.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return fail("missing id"),
    };
    if raw.contains('\0') || raw.contains("..") {
        return fail("invalid id");
    }
    let id = qualify_soundpack_id(&raw, "keyboard/");
    if too_many_per_pack_entries(&id) {
        return fail("too many per-pack entries");
    }
    let v = match clamp_percent(req.get("value")) { Some(v) => v, None => return fail("value must be 0-100") };
    let f = (v / 100.0).clamp(0.0, 1.0);
    crate::state::config_writer::apply(|c| {
        c.per_pack_volume.insert(id.clone(), f);
    });
    let cur = crate::state::config_writer::current();
    if cur.keyboard_soundpack == id {
        let eff = cur.effective_volume();
        engine.send(AudioCommand::SetVolume(eff));
    }
    ok(serde_json::json!({ "id": id, "per_pack_volume": v }))
}

fn delete_pack(req: &serde_json::Value, engine: &AudioEngineHandle) -> String {
    let raw = match req.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return fail("missing id"),
    };
    if raw.contains('\0') || raw.contains("..") || raw.contains('/') && raw.matches('/').count() > 1 {
        return fail("invalid id");
    }
    let id = qualify_soundpack_id(&raw, "keyboard/");
    let name = id.strip_prefix("keyboard/").unwrap_or(&id).to_string();
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return fail("invalid id");
    }
    let base = paths::soundpacks::get_builtin_soundpacks_dir();
    let target = base.join("keyboard").join(&name);
    if !target.join("config.json").exists() {
        return fail("pack not found");
    }
    let canon_base = base.canonicalize().unwrap_or(base.clone());
    let canon_target = target.canonicalize().unwrap_or(target.clone());
    if !canon_target.starts_with(&canon_base) {
        return fail("invalid path");
    }
    if let Err(e) = std::fs::remove_dir_all(&target) {
        return fail(&format!("delete failed: {}", e));
    }
    let mut cache = crate::state::soundpack::SoundpackCache::load();
    cache.soundpacks.remove(&id);
    cache.update_count();
    cache.save();

    let was_active = crate::state::config_writer::current().keyboard_soundpack == id;
    crate::state::config_writer::apply(|c| {
        c.per_pack_volume.remove(&id);
    });
    if was_active {
        let base2 = paths::soundpacks::get_builtin_soundpacks_dir();
        let ids = collect_packs(&base2, "keyboard");
        let next = if ids.is_empty() {
            String::new()
        } else {
            let n = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as usize;
            ids[n % ids.len()].clone()
        };
        let rec = if !next.is_empty() { recommended_volume_for(&next) } else { None };
        crate::state::config_writer::apply(|c| {
            c.keyboard_soundpack = next.clone();
            if !next.is_empty() && !c.per_pack_volume.contains_key(&next) {
                if let Some(v) = rec {
                    let v = v.clamp(0.1, 1.0);
                    if (v - 1.0).abs() > 0.001 {
                        c.per_pack_volume.insert(next.clone(), v);
                    }
                }
            }
        });
        let eff = crate::state::config_writer::current().effective_volume();
        if !next.is_empty() {
            engine.send(AudioCommand::LoadKeyboardPack { soundpack_id: next.clone(), update_cache_on_error: true });
            engine.send(AudioCommand::SetVolume(eff));
        } else {
            engine.send(AudioCommand::SetVolume(crate::state::config_writer::current().volume));
        }
        ok(serde_json::json!({ "deleted": id, "fallback": next }))
    } else {
        ok(serde_json::json!({ "deleted": id }))
    }
}

/// List available packs.
fn packs() -> String {
    let base = paths::soundpacks::get_builtin_soundpacks_dir();
    let mut keyboard: Vec<String> = collect_packs(&base, "keyboard");
    keyboard.sort();
    ok(serde_json::json!({ "keyboard": keyboard }))
}

fn collect_packs(base: &Path, kind: &str) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(base.join(kind)) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let id = format!("{kind}/{name}");
            if e.path().join("config.json").exists() {
                ids.push(id);
            }
        }
    }
    ids
}

fn audio_devices() -> String {
    let dm = crate::libs::device_manager::DeviceManager::new();
    let devices = match dm.get_output_devices() {
        Ok(d) => d,
        Err(e) => return fail(&e),
    };
    let selected = crate::state::config_writer::current().selected_audio_device;
    ok(serde_json::json!({ "devices": devices.iter().map(|d| serde_json::json!({"id": d.id, "name": d.name, "is_default": d.is_default})).collect::<Vec<_>>(), "selected": selected }))
}

fn select_device(req: &serde_json::Value, engine: &AudioEngineHandle) -> String {
    let id = match req.get("id") {
        None | Some(serde_json::Value::Null) => None,
        Some(v) => match v.as_str() {
            Some(s) if !s.is_empty() => Some(s.to_string()),
            Some(_) => None,
            None => return fail("id must be a string or null"),
        },
    };
    if let Some(ref s) = id {
        if s.contains('\0') || s.len() > 256 {
            return fail("invalid id");
        }
    }
    crate::state::config_writer::apply(|c| c.selected_audio_device = id.clone());
    engine.send(AudioCommand::SwitchDevice(id.clone()));
    ok(serde_json::json!({ "selected": id }))
}

fn ok(mut v: serde_json::Value) -> String {
    if let Some(obj) = v.as_object_mut() {
        obj.insert("ok".into(), serde_json::json!(true));
    }
    v.to_string()
}

fn too_many_per_pack_entries(id: &str) -> bool {
    const MAX: usize = 500;
    let c = crate::state::config_writer::current();
    c.per_pack_volume.len() >= MAX && !c.per_pack_volume.contains_key(id)
}

fn proc_kb(key: &str) -> Option<u64> {
    std::fs::read_to_string("/proc/self/status").ok()?.lines().find_map(|l| {
        if l.starts_with(key) { l.split_whitespace().nth(1)?.parse().ok() } else { None }
    })
}

fn diag() -> String {
    let vm_rss = proc_kb("VmRSS:").unwrap_or(0);
    let vm_hwm = proc_kb("VmHWM:").unwrap_or(0);
    let c = crate::state::config_writer::current();
    let cache = crate::state::soundpack::SoundpackCache::load();
    let mut v = serde_json::json!({
        "vm_rss_kb": vm_rss,
        "vm_hwm_kb": vm_hwm,
        "per_pack_volume_entries": c.per_pack_volume.len(),
        "soundpack_cache_entries": cache.soundpacks.len(),
        "keyboard_pack": c.keyboard_soundpack,
    });
    if let Some(obj) = v.as_object_mut() {
        for (k, val) in crate::state::health::snapshot().as_object().cloned().unwrap_or_default() {
            obj.insert(k, val);
        }
    }
    ok(v)
}

fn export_logs() -> String {
    use chrono::Local;
    let contents = crate::utils::log_buffer::export_contents();
    let name = format!("sorakey-log-{}.txt", Local::now().format("%Y%m%d-%H%M%S"));
    ok(serde_json::json!({
        "name": name,
        "contents": contents,
        "lines": crate::utils::log_buffer::len(),
    }))
}

fn get_bar_section() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let path = std::path::PathBuf::from(&home).join(".local/share/sorakey/bar-section");
    if let Ok(s) = std::fs::read_to_string(&path) {
        ok(serde_json::json!({ "section": s.trim() }))
    } else {
        ok(serde_json::json!({ "section": "right" }))
    }
}

fn set_bar_section(req: &serde_json::Value) -> String {
    let Some(section) = req.get("section").and_then(|s| s.as_str()) else { return fail("missing section") };
    if !matches!(section, "left" | "center" | "right") { return fail("invalid section") };
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let dir = std::path::PathBuf::from(home).join(".local/share/sorakey");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return fail(&format!("could not create dir: {e}"));
    }
    let path = dir.join("bar-section");
    if let Err(e) = std::fs::write(&path, section) {
        return fail(&format!("could not write: {e}"));
    }
    ok(serde_json::json!({ "section": section }))
}

fn fail(e: &str) -> String {
    serde_json::json!({ "ok": false, "error": e }).to_string()
}

/// `sorakey ctl '<json>'` client — one request, one response line.
pub fn ctl_client(request: &str) -> i32 {
    let path = socket_path();
    let mut stream = match UnixStream::connect(&path) {
        Ok(s) => s,
        Err(_) => {
            print!("{}", fail("daemon not running"));
            return 1;
        }
    };
    if stream.write_all(request.as_bytes()).is_err() || stream.write_all(b"\n").is_err() {
        print!("{}", fail("write failed"));
        return 1;
    }
    let mut out = String::new();
    {
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
        let mut reader = BufReader::new(&stream);
        if reader.read_line(&mut out).is_err() {
            print!("{}", fail("read failed"));
            return 1;
        }
    }
    print!("{out}");
    0
}

/// `sorakey key <Code> [up]` client — fire-and-forget: write one line and
/// close without waiting for the reply (the server ignores write errors).
pub fn key_client(code: &str, down: bool) -> i32 {
    if code.is_empty() || code.len() > 32 || !code.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return 1;
    }
    let path = socket_path();
    let mut stream = match UnixStream::connect(&path) {
        Ok(s) => s,
        Err(_) => return 1,
    };
    let request = serde_json::json!({ "cmd": "key", "code": code, "down": down }).to_string();
    if stream.write_all(request.as_bytes()).is_err() || stream.write_all(b"\n").is_err() {
        return 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Oversized requests must be rejected, not buffered to OOM.
    #[test]
    fn oversized_requests_are_rejected_not_buffered() {
        let (server, client) = UnixStream::pair().expect("socketpair");
        let mut client = client;

        let filler: String = "a".repeat(70 * 1024);
        let request = format!("{{\"cmd\":\"status\",\"pad\":\"{filler}\"}}\n");
        client
            .write_all(request.as_bytes())
            .expect("client write");

        let (cmd_tx, _cmd_rx) = crossbeam_channel::unbounded::<AudioCommand>();
        let engine = AudioEngineHandle { tx: cmd_tx };

        let server = server;
        server
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .expect("timeout");
        handle_conn(server, &engine);

        let mut response = String::new();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .expect("timeout");
        client.read_to_string(&mut response).expect("read response");
        assert!(
            response.contains("request too large"),
            "oversized request must be rejected, got: {response}"
        );
    }

    /// Normal-sized requests still get a normal response.
    #[test]
    fn normal_requests_are_answered() {
        let (server, client) = UnixStream::pair().expect("socketpair");
        let mut client = client;

        client
            .write_all(b"{\"cmd\":\"status\"}\n")
            .expect("client write");

        let (cmd_tx, _cmd_rx) = crossbeam_channel::unbounded::<AudioCommand>();
        let engine = AudioEngineHandle { tx: cmd_tx };

        let server = server;
        server
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .expect("timeout");
        handle_conn(server, &engine);

        let mut response = String::new();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .expect("timeout");
        client.read_to_string(&mut response).expect("read response");
        assert!(
            response.contains("\"ok\":true"),
            "status must succeed, got: {response}"
        );
    }
}