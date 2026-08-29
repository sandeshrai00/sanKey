//! Unix-socket control API. One JSON line in, one JSON line out, per
//! connection. The daemon is the only writer of state; every mutation goes
//! through `config_writer::apply` plus the engine command, the same two-step
//! the GUI uses, so a socket write can never drift from the hotkey or a
//! future GUI.
//!
//! Socket: `$XDG_RUNTIME_DIR/sankey.sock`

use crate::libs::audio::{ AudioCommand, AudioEngineHandle };
use crate::libs::cli_args::qualify_soundpack_id;
use crate::state::paths;
use std::io::{ BufRead, BufReader, Write };
use std::os::unix::net::{ UnixListener, UnixStream };
use std::path::PathBuf;

pub fn socket_path() -> PathBuf {
    match std::env::var("XDG_RUNTIME_DIR") {
        Ok(dir) => PathBuf::from(dir).join("sankey.sock"),
        Err(_) => PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".sankey.sock"),
    }
}

/// Spawn the accept loop. Returns the bound path (unlinked any stale socket
/// first). The caller holds the returned path to remove on clean exit.
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
    // Restrict socket to owner only (0600)
    let _ = std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600));

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let eng = engine.clone();
                    std::thread::spawn(move || {
                        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
                        handle_conn(stream, &eng)
                    });
                }
                Err(e) => crate::debug_print!("⚠️  [control] accept: {}", e),
            }
        }
    });

    Some(path)
}

fn handle_conn(mut stream: UnixStream, engine: &AudioEngineHandle) {
    let mut line = String::new();
    {
        let cloned = match stream.try_clone() {
            Ok(c) => c,
            Err(_) => return,
        };
        let mut reader = BufReader::new(cloned);
        if reader.read_line(&mut line).is_err() {
            return;
        }
    }
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
        "mute" => {
            let muted = req.get("muted").and_then(|v| v.as_bool()).unwrap_or(true);
            crate::state::config_writer::apply(|c| c.enable_sound = !muted);
            engine.send(AudioCommand::SetSoundEnabled(!muted));
            ok(serde_json::json!({ "muted": muted }))
        }
        "volume" => {
            let v = match clamp_percent(req.get("value")) { Some(v) => v, None => return fail("value must be 0-100") };
            let f = v / 100.0;
            crate::state::config_writer::apply(|c| c.volume = f);
            engine.send(AudioCommand::SetVolume(f));
            ok(serde_json::json!({ "volume": v }))
        }
        "keyboard_pack" => load_pack(&req, engine),
        "packs" => packs(),
        other => fail(&format!("unknown cmd: {other}")),
    }
}

fn clamp_percent(v: Option<&serde_json::Value>) -> Option<f32> {
    let n = v?.as_f64()?;
    if n < 0.0 || n > 100.0 {
        return None;
    }
    Some(n as f32)
}

fn status() -> String {
    let c = crate::state::config_writer::current();
    ok(serde_json::json!({
        "running": true,
        "muted": !c.enable_sound,
        "volume": (c.volume * 100.0).round(),
        "keyboard_pack": c.keyboard_soundpack,
    }))
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

    engine.send(AudioCommand::LoadKeyboardPack { soundpack_id: id.clone(), update_cache_on_error: true });
    crate::state::config_writer::apply(|c| c.keyboard_soundpack = id.clone());
    ok(serde_json::json!({ "id": id }))
}

/// Available pack ids. A pack is a directory containing config.json.
fn packs() -> String {
    let base = paths::soundpacks::get_builtin_soundpacks_dir();
    let mut keyboard: Vec<String> = collect_packs(&base, "keyboard");
    keyboard.sort();
    ok(serde_json::json!({ "keyboard": keyboard }))
}

fn collect_packs(base: &PathBuf, kind: &str) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(base.join(kind)) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let id = format!("{kind}/{name}");
            if e.path().join("config.json").exists() && !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

fn ok(mut v: serde_json::Value) -> String {
    if let Some(obj) = v.as_object_mut() {
        obj.insert("ok".into(), serde_json::json!(true));
    }
    v.to_string()
}

fn fail(e: &str) -> String {
    serde_json::json!({ "ok": false, "error": e }).to_string()
}

/// `sankeyd ctl '<json>'` client: one request, one response line on stdout.
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
        let cloned = match stream.try_clone() {
            Ok(c) => c,
            Err(_) => {
                print!("{}", fail("clone failed"));
                return 1;
            }
        };
        let _ = cloned.set_read_timeout(Some(std::time::Duration::from_secs(5)));
        let mut reader = BufReader::new(cloned);
        if reader.read_line(&mut out).is_err() {
            print!("{}", fail("read failed"));
            return 1;
        }
    }
    print!("{out}");
    0
}