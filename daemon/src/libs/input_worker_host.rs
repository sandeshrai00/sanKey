//! Host side of the Windows input worker. Spawns the worker, reads its
//! events, and feeds the audio engine. Falls back to rdev if it dies.
#![cfg(target_os = "windows")]

use crossbeam_channel::Sender;
use std::io::{ BufRead, BufReader };
use std::os::windows::process::CommandExt;
use std::process::{ Child, Command, Stdio };
use std::sync::atomic::{ AtomicU64, Ordering };
use std::sync::Mutex;
use std::time::{ Duration, Instant };

use crate::libs::input_worker::WORKER_ARG;

/// Hide worker console window.
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Max consecutive failures before giving up.
const MAX_RESTARTS: u32 = 5;

/// Uptime that counts as healthy — resets the failure count.
const HEALTHY_UPTIME: Duration = Duration::from_secs(30);

/// Bumped when enabled keyboards/mice change.
static CONFIG_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Cached enabled-device lists for the reader thread.
static ENABLED_DEVICES: Mutex<Option<(Vec<String>, Vec<String>)>> = Mutex::new(None);

/// Notify the reader thread that the enabled-device lists changed.
pub fn notify_config_changed() {
    let config = crate::state::config_writer::current();
    *ENABLED_DEVICES.lock().unwrap() = Some((config.enabled_keyboards, config.enabled_mice));
    CONFIG_GENERATION.fetch_add(1, Ordering::Release);
}

/// Cached filter for the reader thread.
struct DeviceFilter {
    generation: u64,
    enabled_keyboards: Vec<String>,
    enabled_mice: Vec<String>,
}

impl DeviceFilter {
    fn load() -> Self {
        let generation = CONFIG_GENERATION.load(Ordering::Acquire);
        let (enabled_keyboards, enabled_mice) = ENABLED_DEVICES.lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| {
                let config = crate::state::config_writer::current();
                (config.enabled_keyboards, config.enabled_mice)
            });
        Self { generation, enabled_keyboards, enabled_mice }
    }

    fn refresh_if_stale(&mut self) {
        let generation = CONFIG_GENERATION.load(Ordering::Acquire);
        if generation == self.generation {
            return;
        }
        if let Some((keyboards, mice)) = ENABLED_DEVICES.lock().unwrap().clone() {
            self.enabled_keyboards = keyboards;
            self.enabled_mice = mice;
        }
        self.generation = generation;
    }

    /// Empty list means all devices allowed. Unknown id ("-") also passes through.
    fn allows(&self, kind: char, device_id: &str) -> bool {
        let enabled = match kind {
            'K' => &self.enabled_keyboards,
            _ => &self.enabled_mice,
        };
        enabled.is_empty() || device_id == "-" || enabled.iter().any(|id| id == device_id)
    }
}

/// Tracks Ctrl/Alt for the Ctrl+Alt+M hotkey.
#[derive(Default)]
struct HotkeyState {
    ctrl_pressed: bool,
    alt_pressed: bool,
}

impl HotkeyState {
    /// Returns true if this event was the hotkey and shouldn't play as a normal key.
    fn observe(&mut self, code: &str, is_down: bool) -> bool {
        match code {
            "ControlLeft" | "ControlRight" => {
                self.ctrl_pressed = is_down;
            }
            "AltLeft" | "AltRight" => {
                self.alt_pressed = is_down;
            }
            "KeyM" if is_down && self.ctrl_pressed && self.alt_pressed => {
                return true;
            }
            _ => {}
        }
        false
    }
}

/// Update failure count after one worker run.
fn next_failure_count(failures: u32, uptime: Option<Duration>) -> u32 {
    match uptime {
        Some(uptime) if uptime >= HEALTHY_UPTIME => 0,
        _ => failures + 1,
    }
}

/// Backoff before next restart: 1s, 2s, 4s, 8s capped.
fn restart_delay(failures: u32) -> Duration {
    Duration::from_secs(1u64 << failures.saturating_sub(1).min(3))
}

/// Start the worker supervision thread. Calls `on_fallback` if the worker can't be kept alive.
pub fn start_input_worker_host(
    keyboard_tx: Sender<String>,
    mouse_tx: Sender<String>,
    hotkey_tx: Sender<String>,
    on_fallback: Box<dyn FnOnce() + Send>
) {
    std::thread::spawn(move || {
        let mut failures: u32 = 0;

        loop {
            let started_at = Instant::now();

            match spawn_worker() {
                Ok(child) => {
                    let exit = pump_worker(child, &keyboard_tx, &mouse_tx, &hotkey_tx);

                    if exit == PumpExit::ChannelClosed {
                        return;
                    }

                    failures = next_failure_count(failures, Some(started_at.elapsed()));
                    crate::always_eprint!("⚠️ [InputWorker] Worker exited; restarting");
                }
                Err(e) => {
                    failures = next_failure_count(failures, None);
                    crate::always_eprint!("❌ [InputWorker] Failed to spawn worker: {}", e);
                }
            }

            if failures >= MAX_RESTARTS {
                crate::always_eprint!(
                    "❌ [InputWorker] Giving up after {} attempts - falling back to rdev (keyboard input will not work while the app window is focused)",
                    failures
                );
                on_fallback();
                return;
            }

            std::thread::sleep(restart_delay(failures));
        }
    });
}

#[derive(PartialEq)]
enum PumpExit {
    WorkerGone,
    ReaderFailed,
    ChannelClosed,
}

fn spawn_worker() -> std::io::Result<Child> {
    let exe = std::env::current_exe()?;

    Command::new(exe)
        .arg(WORKER_ARG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
}

/// Forward worker stderr to our log.
fn drain_worker_stderr(stderr: std::process::ChildStderr) {
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            crate::always_eprint!("[InputWorker] {}", line);
        }
    });
}

/// Read worker events and forward to audio channels. Ensures worker is reaped on exit.
fn pump_worker(
    mut child: Child,
    keyboard_tx: &Sender<String>,
    mouse_tx: &Sender<String>,
    hotkey_tx: &Sender<String>
) -> PumpExit {
    let stdin = child.stdin.take();

    if let Some(stderr) = child.stderr.take() {
        drain_worker_stderr(stderr);
    }

    let Some(stdout) = child.stdout.take() else {
        reap(&mut child, stdin);
        return PumpExit::ReaderFailed;
    };

    let mut filter = DeviceFilter::load();
    let mut hotkey = HotkeyState::default();
    let mut exit = PumpExit::WorkerGone;
    // Track held keys so we can synthesize releases if the worker dies mid-press.
    let mut held: Vec<(char, String)> = Vec::new();

    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else {
            exit = PumpExit::ReaderFailed;
            break;
        };

        let Some(event) = parse_worker_line(&line) else {
            continue;
        };

        filter.refresh_if_stale();

        if event.kind == 'K' {
            // Check hotkey before filtering — mute should work from any keyboard.
            if hotkey.observe(event.code, event.is_down) {
                let _ = hotkey_tx.send("TOGGLE_SOUND".to_string());
                continue;
            }
        }

        let Some(wire) = wire_for_event(&event, &filter, &mut held) else {
            continue;
        };

        if event.kind == 'K' {
            crate::libs::trace::record(crate::libs::trace::Point::WorkerSend, event.code, 0.0);
        }

        let sent = match event.kind {
            'K' => keyboard_tx.send(wire),
            _ => mouse_tx.send(wire),
        };
        if sent.is_err() {
            exit = PumpExit::ChannelClosed;
            break;
        }
    }

    if exit != PumpExit::ChannelClosed {
        for (kind, code) in held {
            let wire = format!("UP:{}", code);
            let _ = match kind {
                'K' => keyboard_tx.send(wire),
                _ => mouse_tx.send(wire),
            };
        }
    }

    reap(&mut child, stdin);
    exit
}

/// Ensure worker is gone: close lifeline, kill, wait.
fn reap(child: &mut Child, stdin: Option<std::process::ChildStdin>) {
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
}

struct WorkerEvent<'a> {
    kind: char,
    device_id: &'a str,
    code: &'a str,
    is_down: bool,
}

/// Decide what to send for one event. Filters presses; releases follow held state.
fn wire_for_event(
    event: &WorkerEvent<'_>,
    filter: &DeviceFilter,
    held: &mut Vec<(char, String)>
) -> Option<String> {
    if event.is_down {
        if !filter.allows(event.kind, event.device_id) {
            return None;
        }
        held.push((event.kind, event.code.to_string()));
        return Some(event.code.to_string());
    }

    let is_held = |(kind, code): &(char, String)| *kind == event.kind && code == event.code;
    if !held.iter().any(is_held) {
        return None;
    }
    held.retain(|entry| !is_held(entry));
    Some(format!("UP:{}", event.code))
}

/// Parse one `K\t{device_id}\t{code}\t{down|up}` line.
fn parse_worker_line(line: &str) -> Option<WorkerEvent<'_>> {
    let mut parts = line.split('\t');
    let kind = match parts.next()? {
        "K" => 'K',
        "M" => 'M',
        _ => {
            return None;
        }
    };
    let device_id = parts.next()?;
    let code = parts.next()?;
    let is_down = match parts.next()? {
        "down" => true,
        "up" => false,
        _ => {
            return None;
        }
    };
    if code.is_empty() || parts.next().is_some() {
        return None;
    }

    Some(WorkerEvent { kind, device_id, code, is_down })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_well_formed_lines() {
        let e = parse_worker_line("K\tabc123\tKeyA\tdown").unwrap();
        assert_eq!(e.kind, 'K');
        assert_eq!(e.device_id, "abc123");
        assert_eq!(e.code, "KeyA");
        assert!(e.is_down);

        let e = parse_worker_line("M\t-\tMouseLeft\tup").unwrap();
        assert_eq!(e.kind, 'M');
        assert_eq!(e.device_id, "-");
        assert!(!e.is_down);
    }

    #[test]
    fn rejects_malformed_lines() {
        for line in [
            "",
            "K",
            "K\tabc\tKeyA",
            "X\tabc\tKeyA\tdown",
            "K\tabc\tKeyA\tsideways",
            "K\tabc\t\tdown",
            "K\tabc\tKeyA\tdown\textra",
        ] {
            assert!(parse_worker_line(line).is_none(), "should reject {:?}", line);
        }
    }

    #[test]
    fn empty_filter_allows_everything() {
        let filter = DeviceFilter {
            generation: 0,
            enabled_keyboards: Vec::new(),
            enabled_mice: Vec::new(),
        };
        assert!(filter.allows('K', "anything"));
        assert!(filter.allows('M', "anything"));
    }

    #[test]
    fn populated_filter_allows_only_listed_devices_and_unknown_ids() {
        let filter = DeviceFilter {
            generation: 0,
            enabled_keyboards: vec!["kb1".to_string()],
            enabled_mice: vec!["m1".to_string()],
        };
        assert!(filter.allows('K', "kb1"));
        assert!(!filter.allows('K', "kb2"));
        assert!(filter.allows('M', "m1"));
        assert!(!filter.allows('M', "m2"));
        assert!(filter.allows('K', "-"));
    }

    #[test]
    fn healthy_uptime_clears_accumulated_failures() {
        assert_eq!(next_failure_count(3, Some(HEALTHY_UPTIME)), 0);
        assert_eq!(next_failure_count(MAX_RESTARTS - 1, Some(HEALTHY_UPTIME * 10)), 0);
    }

    #[test]
    fn short_lived_worker_accumulates_failures_until_fallback() {
        let mut failures = 0;
        for expected in 1..=MAX_RESTARTS {
            failures = next_failure_count(failures, Some(Duration::from_secs(1)));
            assert_eq!(failures, expected);
        }
        assert!(failures >= MAX_RESTARTS);
    }

    #[test]
    fn spawn_failure_counts_as_a_failure() {
        assert_eq!(next_failure_count(0, None), 1);
        assert_eq!(next_failure_count(2, None), 3);
    }

    #[test]
    fn healthy_worker_death_retries_immediately_rather_than_underflowing() {
        let failures = next_failure_count(4, Some(HEALTHY_UPTIME));
        assert_eq!(failures, 0);
        assert_eq!(restart_delay(failures), Duration::from_secs(1));
    }

    #[test]
    fn restart_delay_doubles_then_caps() {
        assert_eq!(restart_delay(1), Duration::from_secs(1));
        assert_eq!(restart_delay(2), Duration::from_secs(2));
        assert_eq!(restart_delay(3), Duration::from_secs(4));
        assert_eq!(restart_delay(4), Duration::from_secs(8));
        assert_eq!(restart_delay(5), Duration::from_secs(8));
        assert_eq!(restart_delay(u32::MAX), Duration::from_secs(8));
    }

    fn filter_for(keyboards: Vec<&str>) -> DeviceFilter {
        DeviceFilter {
            generation: 0,
            enabled_keyboards: keyboards.into_iter().map(String::from).collect(),
            enabled_mice: Vec::new(),
        }
    }

    fn event<'a>(device_id: &'a str, code: &'a str, is_down: bool) -> WorkerEvent<'a> {
        WorkerEvent { kind: 'K', device_id, code, is_down }
    }

    #[test]
    fn release_survives_the_device_being_disabled_mid_hold() {
        let mut held = Vec::new();

        let allowed = filter_for(vec![]);
        assert_eq!(wire_for_event(&event("kb1", "KeyX", true), &allowed, &mut held).as_deref(), Some("KeyX"));

        let disabled = filter_for(vec!["kb2"]);
        assert_eq!(
            wire_for_event(&event("kb1", "KeyX", false), &disabled, &mut held).as_deref(),
            Some("UP:KeyX"),
        );
        assert!(held.is_empty());
    }

    #[test]
    fn disabled_device_sends_neither_press_nor_release() {
        let filter = filter_for(vec!["kb1"]);
        let mut held = Vec::new();

        assert!(wire_for_event(&event("kb2", "KeyX", true), &filter, &mut held).is_none());
        assert!(held.is_empty());
        assert!(wire_for_event(&event("kb2", "KeyX", false), &filter, &mut held).is_none());
    }

    #[test]
    fn release_without_a_press_is_dropped() {
        let filter = filter_for(vec![]);
        let mut held = Vec::new();
        assert!(wire_for_event(&event("kb1", "KeyX", false), &filter, &mut held).is_none());
    }

    #[test]
    fn held_tracks_kind_and_code_independently() {
        let filter = filter_for(vec![]);
        let mut held = Vec::new();

        wire_for_event(&event("kb1", "KeyX", true), &filter, &mut held);
        let mouse_down = WorkerEvent { kind: 'M', device_id: "m1", code: "KeyX", is_down: true };
        wire_for_event(&mouse_down, &filter, &mut held);
        assert_eq!(held.len(), 2);

        let mouse_up = WorkerEvent { kind: 'M', device_id: "m1", code: "KeyX", is_down: false };
        assert_eq!(wire_for_event(&mouse_up, &filter, &mut held).as_deref(), Some("UP:KeyX"));
        assert_eq!(held, vec![('K', "KeyX".to_string())]);
    }

    #[test]
    fn hotkey_needs_both_modifiers_held() {
        let mut state = HotkeyState::default();
        assert!(!state.observe("KeyM", true));

        state.observe("ControlLeft", true);
        assert!(!state.observe("KeyM", true));

        state.observe("AltLeft", true);
        assert!(state.observe("KeyM", true));

        state.observe("ControlLeft", false);
        assert!(!state.observe("KeyM", true));
    }
}
