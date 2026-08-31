//! In-memory ring buffer for recent log lines. Shown in Settings and exported on demand.
//! Memory only, fixed size. Never stores key identities.

use std::collections::VecDeque;
use std::sync::atomic::{ AtomicBool, AtomicU64, Ordering };
use std::sync::{ Mutex, OnceLock };

pub const CAPACITY: usize = 2000;
pub const VIEWER_LINES: usize = 100;

static BUFFER: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
static GENERATION: AtomicU64 = AtomicU64::new(0);
static VERBOSE: AtomicBool = AtomicBool::new(false);

fn buffer() -> &'static Mutex<VecDeque<String>> {
    BUFFER.get_or_init(|| Mutex::new(VecDeque::with_capacity(CAPACITY)))
}

/// Current time as HH:MM:SS.mmm.
fn timestamp_now() -> String {
    format_timestamp(chrono::Local::now())
}

fn format_timestamp(at: chrono::DateTime<chrono::Local>) -> String {
    use chrono::Timelike;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        at.hour(),
        at.minute(),
        at.second(),
        at.nanosecond() / 1_000_000
    )
}

/// Append a line, dropping the oldest when full.
pub fn push(line: &str) {
    let Ok(mut buffer) = buffer().lock() else {
        return;
    };

    for part in line.split('\n') {
        if buffer.len() >= CAPACITY {
            buffer.pop_front();
        }
        buffer.push_back(format!("{} {}", timestamp_now(), part.trim_end_matches('\r')));
    }

    drop(buffer);
    GENERATION.fetch_add(1, Ordering::Release);
}

pub fn generation() -> u64 {
    GENERATION.load(Ordering::Acquire)
}

/// Most recent `count` lines, oldest first.
pub fn recent(count: usize) -> Vec<String> {
    let Ok(buffer) = buffer().lock() else {
        return Vec::new();
    };
    let skip = buffer.len().saturating_sub(count);
    buffer.iter().skip(skip).cloned().collect()
}

/// All lines, oldest first.
pub fn snapshot() -> Vec<String> {
    let Ok(buffer) = buffer().lock() else {
        return Vec::new();
    };
    buffer.iter().cloned().collect()
}

pub fn len() -> usize {
    buffer().lock().map(|buffer| buffer.len()).unwrap_or(0)
}

#[inline]
pub fn verbose_enabled() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

/// Toggle verbose capture (not persisted).
pub fn set_verbose(on: bool) {
    VERBOSE.store(on, Ordering::Relaxed);
    crate::libs::trace::set_runtime_tracing(on);
    push(if on {
        "🔊 Verbose logging ON - per-keystroke timings will be captured with key identities masked"
    } else {
        "🔇 Verbose logging OFF"
    });
}

/// Capture a timing line, masking any key identity first.
pub fn push_verbose(line: &str) {
    if !verbose_enabled() {
        return;
    }
    push(&mask_key_identities(line));
}

/// Replace any `key=...` value with `***`.
pub fn mask_key_identities(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;

    while let Some(at) = rest.find("key=") {
        let is_field_start =
            at == 0 || !rest[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_');

        let (head, tail) = rest.split_at(at + "key=".len());
        out.push_str(head);
        rest = tail;

        if !is_field_start {
            continue;
        }

        let value_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        out.push_str("***");
        rest = &rest[value_end..];
    }

    out.push_str(rest);
    out
}

fn current_user_name() -> Option<String> {
    for var in ["USERNAME", "USER", "LOGNAME"] {
        if let Ok(value) = std::env::var(var) {
            let trimmed = value.trim();
            if trimmed.len() >= 3 {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Replace account name in paths with `[username]`.
pub fn mask_user_paths(line: &str) -> String {
    let Some(user) = current_user_name() else {
        return line.to_string();
    };
    mask_name_in_paths(line, &user)
}

fn mask_name_in_paths(line: &str, user: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    let needle = user.to_lowercase();

    loop {
        let haystack = rest.to_lowercase();
        let Some(at) = haystack.find(&needle) else {
            break;
        };

        let before_ok = at == 0 || {
            let prev = rest[..at].chars().next_back();
            matches!(prev, Some('/') | Some('\\'))
        };
        let after = at + needle.len();
        let after_ok =
            after == rest.len() ||
            matches!(rest[after..].chars().next(), Some('/') | Some('\\'));

        out.push_str(&rest[..at]);
        if before_ok && after_ok {
            out.push_str("[username]");
        } else {
            out.push_str(&rest[at..after]);
        }
        rest = &rest[after..];
    }

    out.push_str(rest);
    out
}

pub fn export_header() -> String {
    format!(
        "Sorakey log export\n\
         App version: {}\n\
         OS: {} ({})\n\
         Exported at: {}\n\
         Verbose logging: {}\n\
         Lines captured: {} (buffer holds up to {})\n\
         {}\n",
        crate::utils::constants::APP_VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH,
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        if verbose_enabled() {
            "on (key identities masked)"
        } else {
            "off"
        },
        len(),
        CAPACITY,
        "-".repeat(60)
    )
}

pub fn export_contents() -> String {
    let mut out = mask_user_paths(&export_header());
    for line in snapshot() {
        out.push_str(&mask_user_paths(&line));
        out.push('\n');
    }
    out
}

fn export_file_name(at: chrono::DateTime<chrono::Local>) -> String {
    format!("sorakey-log-{}.txt", at.format("%Y%m%d-%H%M%S"))
}

pub fn export_to_file() -> Result<std::path::PathBuf, String> {
    let dir = std::env::temp_dir().join("sorakey-logs");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create {}: {}", dir.display(), e))?;

    let path = dir.join(export_file_name(chrono::Local::now()));
    std::fs::write(&path, export_contents()).map_err(|e|
        format!("Could not write {}: {}", path.display(), e)
    )?;

    Ok(path)
}

pub fn reveal_in_file_manager(path: &std::path::Path) {
    let spawned = std::process::Command::new("xdg-open")
        .arg(path.parent().unwrap_or(path))
        .spawn();

    if let Err(e) = spawned {
        crate::debug_eprint!("⚠️ Could not open the log folder: {}", e);
    }
}

#[cfg(test)]
pub fn buffer_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reset() {
        buffer().lock().unwrap().clear();
        VERBOSE.store(false, Ordering::Relaxed);
    }

    fn our_indices(marker: &str) -> Vec<usize> {
        snapshot()
            .iter()
            .filter_map(|line| line.split(marker).nth(1)?.trim().parse().ok())
            .collect()
    }

    #[test]
    fn lines_are_kept_in_order_with_a_timestamp() {
        let _guard = super::buffer_test_guard();
        reset();

        push("ordertest 0");
        push("ordertest 1");

        let ours = our_indices("ordertest ");
        assert_eq!(ours, vec![0, 1]);
        let first = snapshot()
            .into_iter()
            .find(|l| l.contains("ordertest 0"))
            .expect("pushed line must be present");
        assert!(first.chars().take(2).all(|c| c.is_ascii_digit()), "{}", first);
    }

    #[test]
    fn the_buffer_rotates_and_never_grows_past_capacity() {
        let _guard = super::buffer_test_guard();
        reset();

        for i in 0..CAPACITY + 50 {
            push(&format!("rotcap {}", i));
        }

        assert_eq!(len(), CAPACITY);
        let ours = our_indices("rotcap ");
        assert!(ours.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(*ours.last().unwrap(), CAPACITY + 49);
        assert!(*ours.first().unwrap() >= 50);
    }

    #[test]
    fn recent_returns_the_tail_oldest_first() {
        let _guard = super::buffer_test_guard();
        reset();

        for i in 0..10 {
            push(&format!("tailtest {}", i));
        }

        let tail = recent(6);
        let ours: Vec<usize> = tail
            .iter()
            .filter_map(|line| line.split("tailtest ").nth(1)?.trim().parse().ok())
            .collect();
        assert!(ours.len() >= 3, "tail must contain our newest lines: {:?}", tail);
        assert!(ours.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(*ours.last().unwrap(), 9);
    }

    #[test]
    fn recent_asking_for_more_than_exists_returns_everything() {
        let _guard = super::buffer_test_guard();
        reset();

        push("loneline 0");
        let tail = recent(VIEWER_LINES);
        assert!(tail.iter().any(|l| l.contains("loneline 0")), "{:?}", tail);
    }

    #[test]
    fn a_multi_line_message_becomes_multiple_entries() {
        let _guard = super::buffer_test_guard();
        reset();

        push("multiline-alpha\nmultiline-beta");

        let lines = snapshot();
        let ours: Vec<&String> = lines.iter().filter(|line| line.contains("multiline-")).collect();

        assert_eq!(ours.len(), 2, "{:?}", lines);
        assert!(ours[0].ends_with("multiline-alpha"), "{}", ours[0]);
        assert!(ours[1].ends_with("multiline-beta"), "{}", ours[1]);
    }

    #[test]
    fn timestamps_are_zero_padded_hms_with_milliseconds() {
        use chrono::TimeZone;

        let at = chrono::Local.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();
        assert_eq!(format_timestamp(at), "03:04:05.000");

        let at = chrono::Local.with_ymd_and_hms(2026, 1, 2, 13, 45, 59).unwrap();
        assert_eq!(format_timestamp(at), "13:45:59.000");
    }

    #[test]
    fn every_push_moves_the_generation() {
        let _guard = super::buffer_test_guard();
        reset();

        let before = generation();
        push("something");
        assert!(generation() > before);
    }

    #[test]
    fn key_identities_are_masked_out_of_trace_lines() {
        let line = "🔬 TRACE key=KeyA         total=     3.1ms  worker->engine=0.2ms";
        let masked = mask_key_identities(line);

        assert!(!masked.contains("KeyA"), "{}", masked);
        assert!(masked.contains("key=***"), "{}", masked);
        assert!(masked.contains("total=     3.1ms"), "{}", masked);
        assert!(masked.contains("worker->engine=0.2ms"), "{}", masked);
    }

    #[test]
    fn masking_covers_every_key_field_on_a_line() {
        let masked = mask_key_identities("key=KeyA then key=ControlLeft done");
        assert_eq!(masked, "key=*** then key=*** done");
    }

    #[test]
    fn masking_leaves_lookalike_fields_alone() {
        let masked = mask_key_identities("monkey=banana");
        assert_eq!(masked, "monkey=banana");
    }

    #[test]
    fn masking_handles_a_key_field_at_end_of_line() {
        assert_eq!(mask_key_identities("timing key=KeyZ"), "timing key=***");
    }

    #[test]
    fn verbose_lines_are_dropped_entirely_while_verbose_is_off() {
        let _guard = super::buffer_test_guard();
        reset();

        push_verbose("🔬 TRACE key=KeyA total=3.1ms");

        assert!(!snapshot().iter().any(|line| line.contains("total=3.1ms")), "{:?}", snapshot());
    }

    #[test]
    fn verbose_lines_reach_the_buffer_masked_when_on() {
        let _guard = super::buffer_test_guard();
        reset();

        VERBOSE.store(true, Ordering::Relaxed);
        push_verbose("🔬 TRACE key=KeyA total=3.1ms");

        let lines = snapshot();
        let ours = lines.iter().find(|line| line.contains("total=3.1ms")).unwrap_or_else(|| panic!("{:?}", lines));

        assert!(!ours.contains("KeyA"), "{}", ours);
        assert!(ours.contains("key=***"), "{}", ours);

        VERBOSE.store(false, Ordering::Relaxed);
    }

    #[test]
    fn no_raw_key_code_can_reach_the_buffer_through_the_verbose_door() {
        let _guard = super::buffer_test_guard();
        reset();
        VERBOSE.store(true, Ordering::Relaxed);

        for code in ["KeyA", "KeyZ", "ControlLeft", "KeyB", "Numpad7"] {
            push_verbose(&format!("🔬 TRACE key={} total=1.0ms", code));
        }

        let joined = snapshot().join("\n");
        for code in ["KeyA", "KeyZ", "ControlLeft", "KeyB", "Numpad7"] {
            assert!(!joined.contains(code), "`{}` leaked:\n{}", code, joined);
        }

        VERBOSE.store(false, Ordering::Relaxed);
    }

    #[test]
    fn the_export_carries_the_header_and_every_line() {
        let _guard = super::buffer_test_guard();
        reset();

        let marker = "export-header-line";
        for i in 0..5 {
            push(&format!("{} {}", marker, i));
        }

        let contents = export_contents();

        assert!(contents.contains("Sorakey log export"), "{}", contents);
        assert!(contents.contains(&format!("App version: {}", crate::utils::constants::APP_VERSION)), "{}", contents);
        assert!(contents.contains(std::env::consts::OS), "{}", contents);
        assert!(contents.contains("Verbose logging: off"), "{}", contents);

        let reported = contents.lines().find_map(|line| line.strip_prefix("Lines captured: ")).and_then(|rest| rest.split_whitespace().next()).and_then(|count| count.parse::<usize>().ok()).unwrap_or_else(|| panic!("{}", contents));
        assert!(reported >= 5);

        for i in 0..5 {
            assert!(contents.contains(&format!("{} {}", marker, i)), "missing {} {}", marker, i);
        }
    }

    #[test]
    fn the_export_hides_the_account_name_in_paths() {
        let _guard = super::buffer_test_guard();
        reset();

        let Some(user) = super::current_user_name() else {
            return;
        };

        push(&format!(r"soundpack_dir: C:\Users\{}\AppData\Local\Sorakey", user));
        push(&format!("config: /home/{}/.config/sorakey/config.json", user));

        let contents = export_contents();

        assert!(!contents.contains(&user), "{}", contents);
        assert!(contents.contains("[username]"), "{}", contents);
        assert!(contents.contains(r"\AppData\Local\Sorakey"), "{}", contents);
        assert!(contents.contains("/.config/sorakey/config.json"), "{}", contents);
    }

    #[test]
    fn masking_replaces_whole_path_components_only() {
        let masked = super::mask_name_in_paths(r"C:\Users\ada\ada-theme\adamant.ogg", "ada");

        assert!(masked.contains(r"C:\Users\[username]\"), "{}", masked);
        assert!(masked.contains("adamant.ogg"), "{}", masked);
        assert!(masked.contains("ada-theme"), "{}", masked);
    }

    #[test]
    fn masking_ignores_case_and_covers_both_path_separators() {
        let masked = super::mask_name_in_paths(r"C:\Users\AroCodes\x and /home/arocodes/y", "arocodes");

        assert!(!masked.to_lowercase().contains("arocodes"), "{}", masked);
        assert_eq!(masked, r"C:\Users\[username]\x and /home/[username]/y");
    }

    #[test]
    fn a_name_at_the_end_of_a_path_is_still_masked() {
        let masked = super::mask_name_in_paths(r"home dir: C:\Users\arocodes", "arocodes");
        assert_eq!(masked, r"home dir: C:\Users\[username]");
    }

    #[test]
    fn the_export_contains_more_than_the_viewer_shows() {
        let _guard = super::buffer_test_guard();
        reset();

        let total = VIEWER_LINES + 250;
        for i in 0..total {
            push(&format!("event {}", i));
        }

        assert_eq!(recent(VIEWER_LINES).len(), VIEWER_LINES);

        let contents = export_contents();
        assert!(contents.contains("event 0"));
        assert!(contents.contains(&format!("event {}", total - 1)));
        let captured: usize = contents.lines().find_map(|l| l.split("Lines captured: ").nth(1)?.split(' ').next()?.parse().ok()).expect("header must report count");
        assert!(captured >= total, "header said {}", captured);
    }

    #[test]
    fn the_export_reports_verbose_state_in_its_header() {
        let _guard = super::buffer_test_guard();
        reset();

        VERBOSE.store(true, Ordering::Relaxed);
        assert!(export_header().contains("Verbose logging: on (key identities masked)"));

        VERBOSE.store(false, Ordering::Relaxed);
        assert!(export_header().contains("Verbose logging: off"));
    }

    #[test]
    fn export_file_names_are_timestamped_and_do_not_collide() {
        use chrono::TimeZone;

        let at = chrono::Local.with_ymd_and_hms(2026, 8, 4, 15, 30, 45).unwrap();
        assert_eq!(export_file_name(at), "sorakey-log-20260804-153045.txt");

        let later = chrono::Local.with_ymd_and_hms(2026, 8, 4, 15, 30, 46).unwrap();
        assert_ne!(export_file_name(at), export_file_name(later));
    }

    #[test]
    fn no_per_event_code_path_logs() {
        const ENGINE: &str = include_str!("../libs/audio/engine.rs");
        const WORKER_HOST: &str = include_str!("../libs/input_worker_host.rs");

        let engine = ENGINE.split("#[cfg(test)]").next().unwrap();
        let worker_host = WORKER_HOST.split("#[cfg(test)]").next().unwrap();

        let logging = ["always_print!", "always_eprint!", "debug_print!", "debug_eprint!"];

        for handler in ["fn handle_key_event("] {
            let body = engine.split(handler).nth(1).expect("handler must exist").split("\n    fn ").next().unwrap();
            for macro_name in logging {
                assert!(!body.contains(macro_name));
            }
        }

        let read_loop = worker_host.split("for line in BufReader::new(stdout).lines()").nth(1).expect("worker read loop must exist").split("\n    reap(").next().unwrap();
        for macro_name in logging {
            assert!(!read_loop.contains(macro_name));
        }
    }

    #[test]
    fn the_verbose_toggle_makes_real_trace_points_reach_the_buffer() {
        use crate::libs::trace::{ Point, record };

        let _guard = super::buffer_test_guard();
        reset();

        assert!(std::env::var("SORAKEY_TRACE").is_err());

        set_verbose(true);

        record(Point::WorkerSend, "KeyA", 0.0);
        record(Point::EngineDequeue, "KeyA", 0.0);
        record(Point::PlayedSound, "KeyA", 0.4);
        record(Point::UiEventSent, "KeyA", 0.0);
        record(Point::UiWrite, "KeyA", 0.0);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut captured = String::new();
        while std::time::Instant::now() < deadline {
            captured = snapshot().join("\n");
            if captured.contains("TRACE") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }

        assert!(captured.contains("TRACE"), "{}", captured);
        assert!(captured.contains("total="), "{}", captured);
        assert!(!captured.contains("KeyA"), "{}", captured);
        assert!(captured.contains("key=***"), "{captured}");

        set_verbose(false);
    }

    #[test]
    fn turning_verbose_off_stops_the_flow() {
        use crate::libs::trace::{ Point, record };

        let _guard = super::buffer_test_guard();
        reset();

        set_verbose(true);
        record(Point::PackLoad, "somepack", 12.0);

        std::thread::sleep(std::time::Duration::from_millis(300));

        set_verbose(false);
        buffer().lock().unwrap().clear();
        let generation_before = generation();

        for _ in 0..200 {
            record(Point::PackLoad, "somepack", 12.0);
        }
        std::thread::sleep(std::time::Duration::from_millis(300));

        assert!(!snapshot().iter().any(|line| line.contains("somepack")), "{:?}", snapshot());
        assert_eq!(generation_before, generation());
    }

    #[test]
    fn typing_with_verbose_off_produces_no_pushes() {
        let _guard = super::buffer_test_guard();
        reset();

        let generation_before = generation();
        for _ in 0..500 {
            push_verbose("🔬 TRACE key=KeyA total=2.0ms");
        }

        assert!(!snapshot().iter().any(|line| line.contains("total=2.0ms")), "{:?}", snapshot());
        assert_eq!(generation(), generation_before);
    }

    #[test]
    #[ignore = "diagnostic: demonstrates the verbose toggle end to end"]
    fn demonstrate_verbose_toggle() {
        use crate::libs::trace::{ Point, record };

        let _guard = super::buffer_test_guard();
        reset();

        println!("SORAKEY_TRACE env var: {:?}", std::env::var("SORAKEY_TRACE").unwrap_or_else(|_| "<unset>".to_string()));

        println!("\n--- toggle OFF, simulating 50 keystrokes ---");
        for _ in 0..50 {
            record(Point::WorkerSend, "KeyA", 0.0);
            record(Point::UiWrite, "KeyA", 0.0);
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
        println!("buffer lines captured: {} (expected 0)", len());

        println!("\n--- toggle ON, simulating 3 keystrokes ---");
        set_verbose(true);
        for key in ["KeyA", "KeyS", "KeyD"] {
            record(Point::WorkerSend, key, 0.0);
            record(Point::EngineDequeue, key, 0.0);
            record(Point::PlayedSound, key, 0.4);
            record(Point::UiEventSent, key, 0.0);
            record(Point::UiWrite, key, 0.0);
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
        println!("buffer now holds {} line(s):", len());
        for line in snapshot() {
            println!("  {}", line);
        }

        println!("\n--- toggle OFF again, simulating 50 more keystrokes ---");
        set_verbose(false);
        let after_off = len();
        for _ in 0..50 {
            record(Point::WorkerSend, "KeyA", 0.0);
            record(Point::UiWrite, "KeyA", 0.0);
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
        println!("lines at toggle-off: {}, lines now: {} (must be equal)", after_off, len());
    }

    #[test]
    #[ignore = "diagnostic: prints a sample export"]
    fn show_a_real_export() {
        let _guard = super::buffer_test_guard();
        reset();

        push("🐛 Debug logging enabled");
        push("🚀 Initializing Sorakey...");
        push("📂 App root (from exe): D:\\sorakey\\target\\release");
        push("🎧 Audio engine thread started");
        push("🎮 Starting Raw Input worker process...");
        VERBOSE.store(true, Ordering::Relaxed);
        push_verbose("🔬 TRACE key=KeyA         total=     3.1ms  worker->engine=0.2ms");

        let path = export_to_file().expect("export must succeed");
        println!("--- exported to: {} ---", path.display());
        println!("{}", std::fs::read_to_string(&path).unwrap());
        println!("--- end ---");

        VERBOSE.store(false, Ordering::Relaxed);
    }

    #[test]
    fn exporting_writes_a_readable_file_containing_the_buffer() {
        let _guard = super::buffer_test_guard();
        reset();

        push("a line that must survive the round trip");

        let path = export_to_file().expect("export must succeed");
        let written = std::fs::read_to_string(&path).expect("exported file must be readable");

        assert!(written.contains("Sorakey log export"));
        assert!(written.contains("a line that must survive the round trip"));

        let _ = std::fs::remove_file(&path);
    }
}
