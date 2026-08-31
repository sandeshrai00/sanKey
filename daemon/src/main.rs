//! sorakey daemon — mechanical keyboard sounds.
//! Forked from MechvibesDX audio core; control via Unix socket plus Ctrl+Alt+M mute.

mod control;
mod libs;
mod state;
mod utils;

use crossbeam_channel::unbounded;

fn main() {
    // `sorakey ctl '<json>'` - client mode, exits after one request.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("ctl") {
        let request = args.get(2).map(String::as_str).unwrap_or("{}");
        std::process::exit(control::ctl_client(request));
    }

    env_logger::init();

    // Only one instance — two would double-play every keystroke.
    let _lock = match acquire_lock() {
        Some(f) => f,
        None => {
            eprintln!("sorakey: already running");
            std::process::exit(1);
        }
    };

    if let Err(e) = state::paths::soundpacks::ensure_soundpack_directories() {
        always_eprint!("⚠️  sorakey: could not create soundpack dirs: {e}");
    }

    let (keyboard_tx, keyboard_rx) = unbounded::<String>();
    let (hotkey_tx, hotkey_rx) = unbounded::<String>();

    let engine = libs::audio::spawn_engine(keyboard_rx, hotkey_rx);

    if let Some(path) = control::serve(engine) {
        always_print!("🔌 sorakey control socket: {}", path.display());
    }

    libs::bootstrap::start_input_capture(keyboard_tx, hotkey_tx);
    always_print!("✅ sorakey ready. Ctrl+Alt+M mutes, Ctrl+C exits.");

    std::thread::park();
}

/// Exclusive lock file — kernel releases it on exit or crash.
fn acquire_lock() -> Option<std::fs::File> {
    let dir = std::env::var("XDG_RUNTIME_DIR").ok()?;
    let path = std::path::PathBuf::from(dir).join("sorakey.lock");
    let file = std::fs::OpenOptions::new().create(true).write(true).open(&path).ok()?;
    let fd = std::os::unix::io::AsRawFd::as_raw_fd(&file);
    let acquired = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) == 0 };
    if acquired {
        Some(file)
    } else {
        None
    }
}