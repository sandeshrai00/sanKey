//! Optional latency tracing (enable with `SORAKEY_TRACE=1`).
//! Records input -> engine -> UI timings on a background thread.

use std::sync::OnceLock;
use std::sync::atomic::{ AtomicBool, Ordering };
use std::time::Instant;

/// Master switch — one relaxed load on the hot path.
static ENABLED: AtomicBool = AtomicBool::new(false);
static ENV_TRACING: AtomicBool = AtomicBool::new(false);
static RUNTIME_TRACING: AtomicBool = AtomicBool::new(false);
static SINK: OnceLock<crossbeam_channel::Sender<Record>> = OnceLock::new();
static ORIGIN: OnceLock<Instant> = OnceLock::new();

/// A hop in the input -> sound -> UI path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Point {
    WorkerSend,
    EngineDequeue,
    PlayedSound,
    UiEventSent,
    UiWrite,
    PackLoad,
    DeviceSwitch,
}

impl Point {
    fn label(self) -> &'static str {
        match self {
            Point::WorkerSend => "worker",
            Point::EngineDequeue => "engine_recv",
            Point::PlayedSound => "sound",
            Point::UiEventSent => "ui_event",
            Point::UiWrite => "ui_write",
            Point::PackLoad => "pack_load",
            Point::DeviceSwitch => "device_switch",
        }
    }
}

/// One observation. Kept Copy and allocation-free for the hot path.
#[derive(Clone, Copy)]
struct Record {
    point: Point,
    at_ms: f64,
    dur_ms: f64,
    key: InlineKey,
}

/// Inline key storage to avoid allocation.
#[derive(Clone, Copy)]
struct InlineKey {
    bytes: [u8; 24],
    len: u8,
}

impl InlineKey {
    fn new(s: &str) -> Self {
        let mut bytes = [0u8; 24];
        let mut end = s.len().min(bytes.len());
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        bytes[..end].copy_from_slice(&s.as_bytes()[..end]);
        Self { bytes, len: end as u8 }
    }

    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("?")
    }
}

#[inline]
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

#[inline]
pub fn now_ms() -> f64 {
    match ORIGIN.get() {
        Some(origin) => origin.elapsed().as_secs_f64() * 1000.0,
        None => 0.0,
    }
}

/// Record one point. No-op when tracing is off.
#[inline]
pub fn record(point: Point, key: &str, dur_ms: f64) {
    if !enabled() {
        return;
    }
    if let Some(tx) = SINK.get() {
        let _ = tx.try_send(Record {
            point,
            at_ms: now_ms(),
            dur_ms,
            key: InlineKey::new(key),
        });
    }
}

/// Time `f` and record it.
#[inline]
pub fn time<T>(point: Point, key: &str, f: impl FnOnce() -> T) -> T {
    if !enabled() {
        return f();
    }
    let started = Instant::now();
    let out = f();
    record(point, key, started.elapsed().as_secs_f64() * 1000.0);
    out
}

fn refresh_enabled() {
    let on = ENV_TRACING.load(Ordering::Relaxed) || RUNTIME_TRACING.load(Ordering::Relaxed);
    ENABLED.store(on, Ordering::Relaxed);
}

/// Start writer thread if not already running.
fn ensure_writer(write_to_console: bool) -> bool {
    if SINK.get().is_some() {
        return true;
    }

    let _ = ORIGIN.set(Instant::now());
    let (tx, rx) = crossbeam_channel::bounded::<Record>(512);
    if SINK.set(tx).is_err() {
        return true;
    }

    let path = write_to_console.then(|| {
        std::env::temp_dir().join(format!("sorakey-trace-{}.log", std::process::id()))
    });

    if let Some(path) = path.as_ref() {
        eprintln!(
            "🔬 [trace] SORAKEY_TRACE=1 - per-keystroke timings below; detail log: {}",
            path.display()
        );
        eprintln!(
            "🔬 [trace] columns: total = worker->ui_write. A hop over {:.0}ms is marked <== SLOW",
            SLOW_HOP_MS
        );
    }

    std::thread::spawn(move || writer_loop(rx, path, write_to_console));
    true
}

/// Enable tracing from `SORAKEY_TRACE=1`. Call once early in main.
pub fn init() {
    let on = std::env::var("SORAKEY_TRACE").map(|v| v == "1").unwrap_or(false);
    if !on {
        return;
    }

    ENV_TRACING.store(true, Ordering::Relaxed);
    ensure_writer(true);
    refresh_enabled();
}

/// Toggle tracing at runtime for the verbose setting.
pub fn set_runtime_tracing(on: bool) {
    if on {
        ensure_writer(ENV_TRACING.load(Ordering::Relaxed));
    }
    RUNTIME_TRACING.store(on, Ordering::Relaxed);
    refresh_enabled();
}

const SLOW_HOP_MS: f64 = 50.0;

/// Pending keystroke being assembled for the console summary.
struct Pending {
    key: InlineKey,
    worker_at: f64,
    engine_at: Option<f64>,
    sound_at: Option<f64>,
    sound_dur: f64,
    ui_event_at: Option<f64>,
}

impl Pending {
    fn summarize(&self, ui_write_at: f64) -> String {
        let engine = self.engine_at.unwrap_or(self.worker_at);
        let sound = self.sound_at.unwrap_or(engine);
        let ui_event = self.ui_event_at.unwrap_or(sound);

        let hops = [
            ("worker->engine", engine - self.worker_at),
            ("handle_key", self.sound_dur.max(sound - engine)),
            ("->ui_event", ui_event - sound),
            ("->ui_write", ui_write_at - ui_event),
        ];
        let total = ui_write_at - self.worker_at;

        let mut line = format!("🔬 TRACE key={:<12} total={:>8.1}ms", self.key.as_str(), total);
        for (name, ms) in hops {
            line.push_str(&format!("  {}={:.1}ms", name, ms));
        }
        if let Some((name, ms)) = hops.iter().copied().find(|(_, ms)| *ms >= SLOW_HOP_MS) {
            line.push_str(&format!("   <== SLOW: {} took {:.0}ms", name, ms));
        }
        line
    }
}

/// Writer thread — owns all formatting and I/O so traced paths don't block.
fn writer_loop(
    rx: crossbeam_channel::Receiver<Record>,
    path: Option<std::path::PathBuf>,
    to_console: bool
) {
    use std::io::Write;

    let mut file = path.as_ref().and_then(|path| std::fs::File::create(path).ok());
    let mut pending: Vec<Pending> = Vec::new();

    while let Ok(rec) = rx.recv() {
        if let Some(f) = file.as_mut() {
            let _ = writeln!(
                f,
                "{:>12.3}\t{}\t{}\t{:.3}",
                rec.at_ms,
                rec.point.label(),
                rec.key.as_str(),
                rec.dur_ms
            );
        }

        let key = rec.key.as_str().to_string();
        let find = |p: &Pending| p.key.as_str() == key;

        match rec.point {
            Point::WorkerSend => {
                pending.retain(|p| !find(p));
                pending.push(Pending {
                    key: rec.key,
                    worker_at: rec.at_ms,
                    engine_at: None,
                    sound_at: None,
                    sound_dur: 0.0,
                    ui_event_at: None,
                });
            }
            Point::EngineDequeue => {
                if let Some(p) = pending.iter_mut().find(|p| find(p)) {
                    p.engine_at = Some(rec.at_ms);
                }
            }
            Point::PlayedSound => {
                if let Some(p) = pending.iter_mut().find(|p| find(p)) {
                    p.sound_at = Some(rec.at_ms);
                    p.sound_dur = rec.dur_ms;
                }
            }
            Point::UiEventSent => {
                if let Some(p) = pending.iter_mut().find(|p| find(p)) {
                    p.ui_event_at = Some(rec.at_ms);
                }
            }
            Point::UiWrite => {
                if let Some(idx) = pending.iter().position(find) {
                    let done = pending.remove(idx);
                    let line = done.summarize(rec.at_ms);
                    if to_console {
                        eprintln!("{}", line);
                    }
                    crate::utils::log_buffer::push_verbose(&line);
                }
            }
            Point::PackLoad => {
                let line = format!(
                    "🔬 TRACE pack_load {} took {:.0}ms",
                    rec.key.as_str(),
                    rec.dur_ms
                );
                if to_console {
                    eprintln!("{}", line);
                }
                crate::utils::log_buffer::push_verbose(&line);
            }
            Point::DeviceSwitch => {
                let line = format!(
                    "🔬 TRACE device_switch {} took {:.0}ms",
                    rec.key.as_str(),
                    rec.dur_ms
                );
                if to_console {
                    eprintln!("{}", line);
                }
                crate::utils::log_buffer::push_verbose(&line);
            }
        }

        if pending.len() > 64 {
            pending.drain(..32);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_key_roundtrips_normal_codes() {
        for code in ["KeyA", "ControlLeft", "KeyB", ""] {
            assert_eq!(InlineKey::new(code).as_str(), code);
        }
    }

    #[test]
    fn inline_key_truncates_without_splitting_a_char() {
        let long = "é".repeat(40);
        let stored = InlineKey::new(&long);
        assert!(stored.as_str().len() <= 24);
        assert!(long.starts_with(stored.as_str()));
        assert_ne!(stored.as_str(), "?");
    }

    static TRACE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn reset_tracing() {
        RUNTIME_TRACING.store(false, Ordering::Relaxed);
        ENV_TRACING.store(false, Ordering::Relaxed);
        refresh_enabled();
    }

    #[test]
    fn tracing_is_off_until_initialized() {
        let _guard = TRACE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_tracing();

        assert!(!enabled());
        record(Point::WorkerSend, "KeyA", 0.0);
        assert_eq!(time(Point::PlayedSound, "KeyA", || 7), 7);
    }

    #[test]
    fn the_runtime_toggle_enables_tracing_without_the_env_var() {
        let _guard = TRACE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_tracing();

        assert!(!enabled());
        assert!(!ENV_TRACING.load(Ordering::Relaxed));

        set_runtime_tracing(true);
        assert!(enabled());
        assert!(SINK.get().is_some());

        reset_tracing();
    }

    #[test]
    fn turning_the_runtime_toggle_off_makes_trace_points_inert_again() {
        let _guard = TRACE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_tracing();

        set_runtime_tracing(true);
        assert!(enabled());

        set_runtime_tracing(false);
        assert!(!enabled());

        record(Point::WorkerSend, "KeyA", 0.0);
        assert_eq!(time(Point::PlayedSound, "KeyA", || 7), 7);

        reset_tracing();
    }

    #[test]
    fn the_env_var_keeps_tracing_on_across_a_runtime_toggle_off() {
        let _guard = TRACE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_tracing();

        ENV_TRACING.store(true, Ordering::Relaxed);
        refresh_enabled();
        assert!(enabled());

        set_runtime_tracing(true);
        assert!(enabled());

        set_runtime_tracing(false);
        assert!(enabled());

        reset_tracing();
        assert!(!enabled());
    }

    #[test]
    fn enabling_twice_reuses_the_one_writer() {
        let _guard = TRACE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_tracing();

        set_runtime_tracing(true);
        let first = SINK.get().expect("writer must exist after the first enable") as *const _;

        set_runtime_tracing(false);
        set_runtime_tracing(true);
        let second = SINK.get().expect("writer must still exist") as *const _;

        assert_eq!(first, second);

        reset_tracing();
    }

    #[test]
    fn summary_marks_the_hop_that_stalled() {
        let pending = Pending {
            key: InlineKey::new("KeyA"),
            worker_at: 0.0,
            engine_at: Some(1.0),
            sound_at: Some(801.0),
            sound_dur: 0.0,
            ui_event_at: Some(801.5),
        };
        let line = pending.summarize(803.0);
        assert!(line.contains("key=KeyA"), "{}", line);
        assert!(line.contains("total=   803.0ms"), "{}", line);
        assert!(line.contains("<== SLOW: handle_key"), "{}", line);
    }

    #[test]
    fn a_clean_keystroke_is_not_flagged() {
        let pending = Pending {
            key: InlineKey::new("KeyB"),
            worker_at: 100.0,
            engine_at: Some(100.1),
            sound_at: Some(100.4),
            sound_dur: 0.3,
            ui_event_at: Some(100.5),
        };
        let line = pending.summarize(109.0);
        assert!(!line.contains("SLOW"), "{}", line);
    }
}
