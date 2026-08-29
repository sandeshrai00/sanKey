//! sankeyd - lean mechanical keyboard sound daemon.
//!
//! Forked from the MechvibesDX (v0.8.2) audio core: same engine, same input
//! capture, minus every window, tray, telemetry and updater. Control goes
//! through the Unix socket (`sankeyd ctl '<json>'`, see control.rs); mute
//! stays the Ctrl+Alt+M hotkey.
//!
//! The fork keeps whole upstream modules (some carry GUI-side helpers the
//! daemon never calls), so dead code is expected and allowed crate-wide.
#![allow(dead_code)]

mod control;
mod libs;
mod state;
mod utils;

use crossbeam_channel::unbounded;

fn main() {
    // `sankeyd ctl '<json>'` - client mode, exits after one request.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("ctl") {
        let request = args.get(2).map(String::as_str).unwrap_or("{}");
        std::process::exit(control::ctl_client(request));
    }

    env_logger::init();

    // One daemon per session: two instances mean two input listeners and two
    // engines, so every keystroke would play twice.
    let _lock = match acquire_lock() {
        Some(f) => f,
        None => {
            eprintln!("sankeyd: already running");
            std::process::exit(1);
        }
    };

    if let Err(e) = state::paths::soundpacks::ensure_soundpack_directories() {
        always_eprint!("⚠️  sankeyd: could not create soundpack dirs: {e}");
    }

    let (keyboard_tx, keyboard_rx) = unbounded::<String>();
    let (hotkey_tx, hotkey_rx) = unbounded::<String>();

    // Engine loads the configured soundpacks itself at startup.
    let engine = libs::audio::spawn_engine(keyboard_rx, hotkey_rx);

    if let Some(path) = control::serve(engine) {
        always_print!("🔌 sankeyd control socket: {}", path.display());
    }

    libs::bootstrap::start_input_capture(keyboard_tx, hotkey_tx);
    always_print!("✅ sankeyd ready. Ctrl+Alt+M mutes, Ctrl+C exits.");

    std::thread::park();
}

/// Exclusive, non-blocking flock for the life of this process. Released by
/// the kernel on any exit, including a crash.
fn acquire_lock() -> Option<std::fs::File> {
    let dir = std::env::var("XDG_RUNTIME_DIR").ok()?;
    let path = std::path::PathBuf::from(dir).join("sankey.lock");
    let file = std::fs::OpenOptions::new().create(true).write(true).open(&path).ok()?;
    let fd = std::os::unix::io::AsRawFd::as_raw_fd(&file);
    let acquired = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) == 0 };
    if acquired {
        Some(file)
    } else {
        None
    }
}