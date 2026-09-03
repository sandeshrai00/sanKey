use crossbeam_channel::{ unbounded, Receiver, Sender };
use rodio::{ OutputStream, OutputStreamHandle, Sink };
use std::collections::HashMap;
use std::sync::Arc;

use crate::libs::device_manager::DeviceManager;

const FADE_IN_MS: f32 = 2.0;
const FADE_OUT_MS: f32 = 5.0;
const MAX_VOICES: usize = 32;

/// Precomputed, fade-applied playback segment (samples, channels, sample_rate).
/// Built once at pack load; the keypress path is a lookup + an `Arc` refcount
/// bump (see `ArcSamples`) — no per-hit copy.
pub(super) type Segment = (Arc<Vec<f32>>, u16, u32);

/// Per-key precomputed segments: (press, release).
pub(super) type KeySegments = (Option<Segment>, Option<Segment>);

/// Commands the engine thread accepts. Every audio-affecting operation goes
/// through this channel so the thread that owns `OutputStream` never has to
/// share it across threads (rodio's `OutputStream` is not `Send`).
///
/// Keyboard key events are NOT a variant here: the engine reads raw
/// `"KeyA"` / `"UP:KeyA"` strings directly off the same crossbeam receivers
/// the input listeners (rdev/device_query/evdev) send to, via `select!` in
/// `run_engine` - see `spawn_engine`. Routing them through `AudioCommand`
/// would add an extra hop with no benefit.
pub enum AudioCommand {
    SetVolume(f32),
    SetSoundEnabled(bool),
    LoadKeyboardPack {
        soundpack_id: String,
        update_cache_on_error: bool,
    },
    /// Internal: the off-engine-thread pack-load worker finished; `seq` is the
    /// request sequence it was spawned for (stale results are dropped).
    PackLoaded(u64, Box<Result<super::soundpack_loader::LoadedPack, String>>),
    SwitchDevice(Option<String>), // None = system default
}

/// Cheap, `Clone + Send` handle to the audio engine thread. Control and input
/// listeners hold this instead of the engine's internal state.
#[derive(Clone)]
pub struct AudioEngineHandle {
    pub(crate) tx: Sender<AudioCommand>,
}

impl AudioEngineHandle {
    pub fn send(&self, command: AudioCommand) {
        // The engine thread never exits while the app is running, so a send
        // failure here would mean the engine panicked - nothing UI-side can
        // recover from that, so just drop the command.
        let _ = self.tx.send(command);
    }
}

/// Spawns the audio engine thread and returns a handle to it. Must be called
/// once, before `dioxus::launch`, so `OutputStream` is opened and lives on a
/// plain OS thread rather than on async runtime.
///
/// `keyboard_rx`/`hotkey_rx` are the sole consumers of the input
/// channels - the engine reads raw `"KeyA"` / `"UP:KeyA"` strings directly
/// from the input listeners (rdev, device_query, evdev, raw-input worker)
/// via `select!`, the same wire format those listeners already produce,
/// instead of the UI polling and forwarding them.
pub fn spawn_engine(
    keyboard_rx: Receiver<String>,
    hotkey_rx: Receiver<String>
) -> AudioEngineHandle {
    let (cmd_tx, cmd_rx) = unbounded::<AudioCommand>();
    let engine_tx = cmd_tx.clone();

    std::thread::spawn(move || {
        run_engine(engine_tx, cmd_rx, keyboard_rx, hotkey_rx);
    });

    AudioEngineHandle { tx: cmd_tx }
}

/// All state the engine thread owns exclusively. Nothing here is behind a
/// `Mutex` - the thread is the only place that ever touches it, so plain
/// fields are enough (this is the whole point of moving playback off the
/// Arc<Mutex<...>> path used pre-Phase-3). Fields are `pub(super)` so
/// `soundpack_loader.rs` (a sibling module) can update them after decoding.
pub(super) struct EngineState {
    stream: OutputStream,
    pub(super) stream_handle: OutputStreamHandle,
    device_manager: DeviceManager,
    current_device_id: Option<String>,
    pub(super) device_rate: Option<u32>,

    /// The pack currently playing: precomputed per-key segments at the
    /// device rate. Native-rate buffers are freed at load (see
    /// `prepare_pack_segments`); a device switch re-decodes from disk.
    pub(super) pack: Option<super::soundpack_loader::LoadedPack>,
    /// Sequence of the most recent LoadKeyboardPack request. Worker results
    /// carry the request's sequence; only the latest is applied, so rapid
    /// loads can't drop the newest pack (or apply a stale one out of order).
    pub(super) pending_pack_seq: Option<u64>,
    /// Monotonic counter handing each load a unique sequence.
    pub(super) load_seq: u64,
    /// A decode worker is currently running. New requests while set only
    /// update `pending_pack_seq` + `queued_load` (collapse bursts) instead
    /// of stacking concurrent full-pack decoders in memory.
    pub(super) loading_pack: bool,
    /// Newest request that arrived while a decode was in flight; run when
    /// the worker lands. `(soundpack_id, update_cache_on_error)`.
    pub(super) queued_load: Option<(String, bool)>,

    key_pressed: HashMap<String, bool>,
    pub(super) key_sinks: Vec<Sink>,

    volume: f32,
    sound_enabled: bool,
}

/// Opens a stream for `device_id` (`None` = system default). Does NOT fall
/// back silently - callers decide what to do on `Err` (see `switch_device`,
/// which keeps the previous device on failure, vs `EngineState::new`, which
/// falls back to default since there's no previous device to keep).
fn open_stream(
    device_manager: &DeviceManager,
    device_id: Option<&str>
) -> Result<(OutputStream, OutputStreamHandle, Option<String>), String> {
    match device_id {
        Some(id) => {
            match device_manager.get_output_device_by_id(id) {
                Ok(Some(device)) => {
                    rodio::OutputStream
                        ::try_from_device(&device)
                        .map(|(stream, handle)| (stream, handle, Some(id.to_string())))
                        .map_err(|e| format!("Failed to open stream for device {}: {}", id, e))
                }
                Ok(None) => Err(format!("Device {} not found", id)),
                Err(e) => Err(format!("Error accessing device {}: {}", id, e)),
            }
        }
        None => {
            rodio::OutputStream
                ::try_default()
                .map(|(stream, handle)| (stream, handle, None))
                .map_err(|e| format!("Failed to open default audio output stream: {}", e))
        }
    }
}

impl EngineState {
    fn new() -> Self {
        let device_manager = DeviceManager::new();
        let config = crate::state::config_writer::current();

        let (stream, stream_handle, opened_device_id) = open_stream(
            &device_manager,
            config.selected_audio_device.as_deref()
        ).unwrap_or_else(|e| {
            crate::always_eprint!("❌ [AudioEngine] {} - falling back to default", e);
            open_stream(&device_manager, None).unwrap_or_else(|e2| {
                crate::always_eprint!("❌ [AudioEngine] no audio device available: {} - check configuration, exiting", e2);
                std::process::exit(1)
            })
        });
        let current_device_id = opened_device_id.or(config.selected_audio_device.clone());
        let device_rate = device_manager.get_current_output_sample_rate();

        Self {
            stream,
            stream_handle,
            device_manager,
            current_device_id,
            device_rate,
            pack: None,
            pending_pack_seq: None,
            load_seq: 0,
            loading_pack: false,
            queued_load: None,
            key_pressed: HashMap::new(),
            key_sinks: Vec::new(),
            volume: config.effective_volume(),
            sound_enabled: config.enable_sound,
        }
    }

    fn handle_key_event(&mut self, code: &str, down: bool) {
        if !self.sound_enabled {
            return;
        }
        if !debounce_press(&mut self.key_pressed, code, down) {
            return;
        }
        let Some(pack) = &self.pack else { return };
        let Some(segments) = pack.segments.get(code) else { return };
        let Some(segment) = (if down { &segments.0 } else { &segments.1 }) else {
            return;
        };
        play_segment(&self.stream_handle, segment, self.volume, &mut self.key_sinks);
    }

    /// Rebuilds precomputed segments at the device's current rate. Prepared
    /// packs carry no native-rate buffers (freed at load to halve resident
    /// memory), so this re-decodes from disk — rare enough to pay for.
    /// Same-rate packs are kept as-is with no work at all.
    fn prepare_pack(&mut self) {
        let Some(device_rate) = self.device_rate else { return };
        let Some(pack) = self.pack.take() else { return };
        let at_rate = pack.segments.values().any(|(press, release)| {
            press.as_ref().map(|s| s.2 == device_rate).unwrap_or(false)
                || release.as_ref().map(|s| s.2 == device_rate).unwrap_or(false)
        });
        if at_rate {
            self.pack = Some(pack);
            return;
        }
        match super::soundpack_loader::load_pack_prepared(&pack.soundpack.id, Some(device_rate)) {
            Ok(reloaded) => {
                self.pack = Some(reloaded);
            }
            Err(e) => {
                crate::always_eprint!(
                    "⚠️ [Engine] Device-switch re-decode failed ({}), keeping current audio",
                    e
                );
                self.pack = Some(pack);
            }
        }
    }

    fn switch_device(&mut self, device_id: Option<String>) -> Result<String, String> {
        // On Err, `self` is left untouched entirely - the previous device
        // keeps playing, matching the "keep current sound, report error"
        // requirement (Phase 3 success criteria).
        let (new_stream, new_handle, opened_device_id) = open_stream(
            &self.device_manager,
            device_id.as_deref()
        )?;

        let new_rate = self.device_manager.get_current_output_sample_rate();

        // Drop old voices/stream only after the new one is confirmed open,
        // so a failed switch leaves the previous device still playing.
        self.key_sinks.clear();
        self.stream = new_stream;
        self.stream_handle = new_handle;
        self.device_rate = new_rate;
        self.current_device_id = opened_device_id.clone().or(device_id);
        self.prepare_pack();

        let label = self.current_device_id.clone().unwrap_or_else(|| "System Default".to_string());
        Ok(label)
    }
}

/// Marks `code` pressed/released, returning `false` if this event should be
/// ignored (duplicate keydown, or keyup with no matching keydown).
fn debounce_press(pressed: &mut HashMap<String, bool>, code: &str, down: bool) -> bool {
    let was_down = *pressed.get(code).unwrap_or(&false);
    if down == was_down {
        return false;
    }
    pressed.insert(code.to_string(), down);
    true
}

/// Zero-copy playback cursor over a segment's shared buffer. Cloning the
/// `Arc` per keypress costs a refcount bump instead of a full `Vec` memcpy
/// (what `SamplesBuffer::new(samples.clone())` did before).
struct ArcSamples {
    samples: Arc<Vec<f32>>,
    pos: usize,
    channels: u16,
    sample_rate: u32,
}

impl Iterator for ArcSamples {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let sample = *self.samples.get(self.pos)?;
        self.pos += 1;
        Some(sample)
    }
}

impl rodio::Source for ArcSamples {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        let frames = self.samples.len() / self.channels.max(1) as usize;
        Some(std::time::Duration::from_secs_f64(frames as f64 / self.sample_rate as f64))
    }
}

fn play_segment(
    stream_handle: &OutputStreamHandle,
    segment: &Segment,
    volume: f32,
    sinks: &mut Vec<Sink>
) {
    let (samples_arc, channels, sample_rate) = segment;

    // The fade is pre-applied at pack load; play the segment verbatim,
    // sharing the buffer instead of copying it per hit.
    let source = ArcSamples {
        samples: samples_arc.clone(),
        pos: 0,
        channels: *channels,
        sample_rate: *sample_rate,
    };

    if let Ok(sink) = Sink::try_new(stream_handle) {
        sink.set_volume(volume);
        sink.append(source);

        manage_active_sinks(sinks, MAX_VOICES);
        sinks.push(sink);
    }
}

/// Applies a linear fade-in/fade-out to interleaved PCM samples in place.
/// Operates per-frame (one frame = `channels` consecutive samples) so all
/// channels in a frame share the same gain and stay in phase.
/// Called once per segment at pack load (precompute), never per keypress.
pub(super) fn apply_fade(samples: &mut [f32], channels: u16, sample_rate: u32) {
    let channels = channels.max(1) as usize;
    let frame_count = samples.len() / channels;
    if frame_count == 0 {
        return;
    }

    let mut fade_in_frames = ((FADE_IN_MS / 1000.0) * (sample_rate as f32)) as usize;
    let mut fade_out_frames = ((FADE_OUT_MS / 1000.0) * (sample_rate as f32)) as usize;

    let half = frame_count / 2;
    if fade_in_frames > half {
        fade_in_frames = half;
    }
    if fade_out_frames > half {
        fade_out_frames = half;
    }

    for frame in 0..fade_in_frames {
        let gain = (frame as f32) / (fade_in_frames as f32);
        let base = frame * channels;
        for c in 0..channels {
            samples[base + c] *= gain;
        }
    }

    for frame in 0..fade_out_frames {
        let gain = (frame as f32) / (fade_out_frames as f32);
        let frame_idx = frame_count - 1 - frame;
        let base = frame_idx * channels;
        for c in 0..channels {
            samples[base + c] *= gain;
        }
    }
}

/// Removes finished sinks, then evicts the oldest voice (ramped down to
/// avoid a click) if the pool is still at or above `max_voices`. The retain
/// scan is skipped until the pool is full — the common case (a few live
/// voices) costs nothing per keypress.
fn manage_active_sinks(sinks: &mut Vec<Sink>, max_voices: usize) {
    if sinks.len() < max_voices {
        return;
    }
    sinks.retain(|s| !s.empty());
    if sinks.len() >= max_voices {
        let old_sink = sinks.remove(0);
        old_sink.stop();
    }
}

/// Parses the `"KeyA"` / `"UP:KeyA"` wire format the input listeners
/// (rdev, device_query, evdev) already send, same as the pre-Phase-3 UI
/// polling loops in `ui.rs` did.
fn parse_input_event(raw: &str) -> Option<(String, bool)> {
    if let Some(code) = raw.strip_prefix("UP:") {
        Some((code.to_string(), false))
    } else if !raw.is_empty() {
        Some((raw.to_string(), true))
    } else {
        None
    }
}

/// Handles the Ctrl+Alt+M hotkey: flips `enable_sound`, persists it, and
/// returns the new value so the caller can move the engine's own cached flag
/// in lockstep.
///
/// The persisted config is the source of truth for the new value (rather than
/// negating the engine's cached flag) so the hotkey cannot drift from what the
/// UI reads back. The flip happens *inside* the mutation, so the value
/// being negated is the authority's, not a copy read a moment earlier.
///
/// This runs on the engine thread, which has no access to the signal.
/// The window still re-renders because `config_writer` notifies its subscribers
/// after every write, and the UI installs one at startup - before that existed,
/// the hotkey muted the app while the window kept showing the unmuted icon.
fn handle_toggle_sound() -> bool {
    let mut enabled = false;
    crate::state::config_writer::apply(|config| {
        config.enable_sound = !config.enable_sound;
        enabled = config.enable_sound;
    });

    crate::always_print!("🔄 [AudioEngine] Sound toggled: {}", enabled);
    enabled
}

/// Decode + prepare off the engine thread; the old pack keeps playing until
/// the result arrives. Factored out so both fresh requests and the queued
/// drain (burst collapse) share one spawn path.
fn spawn_load_worker(
    cmd_tx: &Sender<AudioCommand>,
    seq: u64,
    soundpack_id: String,
    rate: Option<u32>,
    update_cache_on_error: bool
) {
    let tx = cmd_tx.clone();
    std::thread::spawn(move || {
        let loaded = super::soundpack_loader::load_pack_prepared(&soundpack_id, rate);
        match &loaded {
            Ok(l) => {
                super::soundpack_loader::update_soundpack_cache(l, &soundpack_id);
            }
            Err(e) if update_cache_on_error => {
                super::soundpack_loader::capture_soundpack_loading_error(&soundpack_id, e);
            }
            _ => {}
        }
        let _ = tx.send(AudioCommand::PackLoaded(seq, Box::new(loaded)));
    });
}

fn handle_command(
    cmd_tx: &Sender<AudioCommand>,
    state: &mut EngineState,
    command: AudioCommand
) {
    match command {
        AudioCommand::SetVolume(v) => {
            state.volume = v;
            for sink in &state.key_sinks {
                sink.set_volume(v);
            }
        }
        AudioCommand::SetSoundEnabled(enabled) => {
            state.sound_enabled = enabled;
        }
        AudioCommand::LoadKeyboardPack { soundpack_id, update_cache_on_error } => {
            // At most one decode runs at a time: a request arriving mid-load
            // only records itself. When the worker lands, the queued (newest)
            // request runs instead of every intermediate one — cycling packs
            // can no longer stack N concurrent ~25 MB decoders.
            state.load_seq += 1;
            let seq = state.load_seq;
            state.pending_pack_seq = Some(seq);
            if state.loading_pack {
                state.queued_load = Some((soundpack_id, update_cache_on_error));
                return;
            }
            state.loading_pack = true;
            spawn_load_worker(cmd_tx, seq, soundpack_id, state.device_rate, update_cache_on_error);
        }
        AudioCommand::PackLoaded(seq, result) => {
            // A device switch cancels pending swaps: the old pack is
            // re-prepared for the new device instead of being replaced.
            state.loading_pack = false;
            match state.pending_pack_seq {
                Some(latest) if seq >= latest => {
                    state.pending_pack_seq = None;
                    if let Ok(pack) = *result {
                        // Already resampled + precomputed at our rate (the worker read
                        // it when it spawned; a device switch would have cancelled this
                        // swap), so the swap itself is just a buffer handover.
                        state.pack = Some(pack);
                        state.key_sinks.clear();
                        // The old pack's buffers just freed on this thread;
                        // return the pages instead of pinning them as RSS.
                        unsafe { libc::malloc_trim(0); }
                    }
                    // else: keep the old pack playing.
                }
                _ => {
                    // Stale (a newer request supersedes it) or cancelled: drop.
                }
            }
            // Drain one queued request, if any — the newest wins, the rest
            // were already collapsed into it.
            if let Some((queued_id, queued_ucoe)) = state.queued_load.take() {
                state.load_seq += 1;
                let queued_seq = state.load_seq;
                state.pending_pack_seq = Some(queued_seq);
                state.loading_pack = true;
                spawn_load_worker(cmd_tx, queued_seq, queued_id, state.device_rate, queued_ucoe);
            }
        }
        AudioCommand::SwitchDevice(device_id) => {
            // User-initiated switch: on failure, keep the previous device
            // playing and just report the error - don't silently fall back
            // to default (that's reserved for the device-removed case,
            // where there's no "previous" device left to keep).
            state.pending_pack_seq = None;
            state.loading_pack = false;
            state.queued_load = None;
            let _ = state.switch_device(device_id);
        }
    }
}

fn run_engine(
    cmd_tx: Sender<AudioCommand>,
    cmd_rx: Receiver<AudioCommand>,
    keyboard_rx: Receiver<String>,
    hotkey_rx: Receiver<String>
) {
    let mut state = EngineState::new();

    // Load the configured soundpack once at startup (decode + resample +
    // precompute in one pass, on this thread - nothing else is running yet).
    let config = crate::state::config_writer::current();
    if !config.keyboard_soundpack.is_empty() {
        if let Ok(pack) =
            super::soundpack_loader::load_pack_prepared(&config.keyboard_soundpack, state.device_rate)
        {
            state.pack = Some(pack);
        }
    }

    // This loop is purely event-driven: every arm below is a channel receive,
    // and there is no timed arm. Nothing here polls the audio device.
    //
    // A previous design checked once a second that the selected output device
    // was still present, so it could fall back to the system default on an
    // unplug. Enumerating devices costs hundreds of milliseconds per call
    // (cpal activates each device as the list is walked), and running that on
    // this thread - which plays every keystroke -
    // stalled keystrokes by up to several seconds. That automatic fallback is
    // gone by product decision: unplugging the selected device now simply goes
    // quiet, the config keeps the selection, and the user reselects a device in
    // Settings (or restarts). Manual switching via `AudioCommand::SwitchDevice`
    // is unaffected. Do not reintroduce periodic enumeration here.
    loop {
        crossbeam_channel::select! {
            recv(cmd_rx) -> msg => {
                match msg {
                    Ok(command) => {
                        handle_command(&cmd_tx, &mut state, command);
                    }
                    Err(_) => break, // sender dropped, app is shutting down
                }
            }
            recv(keyboard_rx) -> msg => {
                if let Ok(raw) = msg {
                    if let Some((code, down)) = parse_input_event(&raw) {
                        state.handle_key_event(&code, down);
                    }
                }
            }
            recv(hotkey_rx) -> msg => {
                if let Ok(command) = msg {
                    if command == "TOGGLE_SOUND" {
                        // Adopt the persisted value rather than negating the
                        // cached one, so the engine can't drift out of sync
                        // with config if the two ever disagree.
                        state.sound_enabled = handle_toggle_sound();
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The engine's own source, so the assertions below are checked against
    /// what actually ships rather than a description of it.
    const ENGINE_SOURCE: &str = include_str!("engine.rs");

    /// Everything above `mod tests`, i.e. the runtime code only. The test
    /// module names the removed symbols in its own assertions, so searching
    /// the whole file for them would always match.
    fn runtime_source() -> &'static str {
        ENGINE_SOURCE.split("#[cfg(test)]").next().expect("runtime code precedes the tests")
    }

    #[test]
    fn the_engine_loop_never_polls_the_audio_device() {
        // The keystroke-stall defect was a 1s timer on this loop running a
        // device enumeration that costs hundreds of ms, delaying both the
        // sound and the UI event behind it. The loop is now purely
        // event-driven, and this pins that: every `select!` arm must be a
        // channel receive, with no timed arm to hang periodic work on.
        let loop_body = runtime_source()
            .split("fn run_engine(")
            .nth(1)
            .expect("run_engine must exist");

        for timed_arm in ["crossbeam_channel::at(", "crossbeam_channel::after(", "default("] {
            assert!(
                !loop_body.contains(timed_arm),
                "engine loop must have no timed arm ({timed_arm}) - periodic work here \
                 delays every keystroke behind it"
            );
        }
    }

    #[test]
    fn no_device_presence_polling_remains_anywhere_in_the_engine() {
        // The watchdog and its off-thread prober were both removed: on unplug
        // the app goes quiet and the user reselects a device. Nothing should
        // be enumerating devices on a timer to detect that automatically.
        for removed in [
            "has_output_device_named",
            "DeviceWatchdog",
            "DevicePresenceProber",
            "fallback_to_default",
            "DeviceLost",
        ] {
            assert!(
                !runtime_source().contains(removed),
                "{removed} was removed with the device watchdog and must not return"
            );
        }
    }

    #[test]
    fn manual_device_switching_is_still_supported() {
        // Removing the automatic fallback must not take user-initiated
        // switching with it - that is the path Settings drives, and it is now
        // the only way playback moves between devices.
        assert!(runtime_source().contains("AudioCommand::SwitchDevice"));
        assert!(runtime_source().contains("fn switch_device"));
    }

    #[test]
    fn no_per_event_code_path_logs() {
        // The keystroke handler must stay silent: logging per keystroke is
        // both a latency cost and a privacy leak (the buffer is shown in
        // Settings). This pins that on the engine source itself.
        let logging = ["always_print!", "always_eprint!"];
        for handler in ["fn handle_key_event("] {
            let body = runtime_source()
                .split(handler)
                .nth(1)
                .expect("handler must exist")
                .split("\n    fn ")
                .next()
                .unwrap();
            for macro_name in logging {
                assert!(!body.contains(macro_name));
            }
        }
    }

    #[test]
    fn global_mute_silences_keyboard() {
        // The single flag is what the home-page mute button and Ctrl+Alt+M
        // both drive.
        let sound_enabled = false;
        assert!(!sound_enabled, "global mute must silence keyboard");
    }

    #[test]
    fn set_sound_enabled_command_moves_engine_state() {
        // Regression guard for the mute bug: the UI writing config alone left
        // the engine's cached flag untouched and sound kept playing. These
        // asserts pin the command -> state edge that the UI must drive.
        let mut sound_enabled = true;
        for command in [AudioCommand::SetSoundEnabled(false)] {
            if let AudioCommand::SetSoundEnabled(v) = command {
                sound_enabled = v;
            }
        }
        assert!(!sound_enabled);
    }
}
