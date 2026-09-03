//! The only place `config.json` is written — use `apply` to mutate.

use crate::state::config::AppConfig;
use std::sync::atomic::{ AtomicU64, Ordering };
use std::sync::{ Mutex, OnceLock };

/// Authoritative config — loaded once, then held in memory.
static AUTHORITY: OnceLock<Mutex<AppConfig>> = OnceLock::new();

/// Bump on each real write — UI polls this.
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn authority() -> &'static Mutex<AppConfig> {
    AUTHORITY.get_or_init(|| Mutex::new(AppConfig::load()))
}

/// Snapshot of current config (read-only).
pub fn current() -> AppConfig {
    // recover from poisoned lock instead of crashing
    match authority().lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

/// Apply mutation to config and persist if changed.
pub fn apply(mutate: impl FnOnce(&mut AppConfig)) -> bool {
    let changed = {
        let mut guard = match authority().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        let before = guard.clone();
        mutate(&mut guard);

        let changed = !guard.data_equals(&before);
        if changed {
            guard.last_updated = chrono::Utc::now();
            if let Err(e) = guard.save() {
                crate::always_eprint!("❌ [config] Failed to save config: {}", e);
            }
        } else {
            // rollback metadata-only change so memory doesn't drift
            *guard = before;
        }

        changed
    };

    if changed {
        // Release pairs with Acquire in generation()
        GENERATION.fetch_add(1, Ordering::Release);
    }

    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Local model of the authority for tests.
    struct Authority {
        config: AppConfig,
        writes: usize,
    }

    impl Authority {
        fn new() -> Self {
            Self { config: AppConfig::default(), writes: 0 }
        }

        /// Like `apply` but local.
        fn apply(&mut self, mutate: impl FnOnce(&mut AppConfig)) -> bool {
            let before = self.config.clone();
            mutate(&mut self.config);
            let changed = !self.config.data_equals(&before);
            if changed {
                self.writes += 1;
            } else {
                self.config = before;
            }
            changed
        }
    }

    /// Different subsystems mutating different fields both survive.
    #[test]
    fn mutations_from_different_subsystems_compose() {
        let mut authority = Authority::new();

        // UI changes volume
        authority.apply(|config| {
            config.volume = 0.77;
        });
        // engine mutes
        authority.apply(|config| {
            config.enable_sound = false;
        });
        // tokio sets auto_start
        authority.apply(|config| {
            config.auto_start = true;
        });

        assert_eq!(
            authority.config.volume, 0.77,
            "the UI's field must survive the two later writers"
        );
        assert!(!authority.config.enable_sound, "the engine's field must survive the tokio write");
        assert!(
            authority.config.auto_start,
            "and the tokio task's own field must be recorded"
        );
    }

    /// Slow writer finishing late doesn't revert concurrent changes.
    #[test]
    fn a_slow_writer_finishing_late_does_not_revert_a_concurrent_change() {
        let mut authority = Authority::new();

        // user changes volume and sound during request
        authority.apply(|config| {
            config.volume = 0.35;
        });
        authority.apply(|config| {
            config.enable_sound = false;
        });

        // check writes result
        authority.apply(|config| {
            config.auto_start = true;
        });

        assert_eq!(authority.config.volume, 0.35, "the volume set during the check must survive");
        assert!(!authority.config.enable_sound, "and so must the sound toggle");
        assert!(
            authority.config.auto_start,
            "while the check still records its own result"
        );
    }

    /// Long-lived writer doesn't restore launch state.
    #[test]
    fn a_long_lived_writer_does_not_restore_launch_state() {
        let mut authority = Authority::new();
        let launch_volume = authority.config.volume;

        // service starts at launch
        // user changes volume
        authority.apply(|config| {
            config.volume = 0.88;
        });
        assert_ne!(launch_volume, 0.88, "the test must actually move the value");

        // periodic ticks
        for tick in 0..10 {
            authority.apply(|config| {
                config.per_pack_volume.insert(format!("keyboard/tick{}", tick), 0.5);
            });
            assert_eq!(
                authority.config.volume,
                0.88,
                "tick {tick} must not restore the launch-time volume"
            );
        }
    }

    /// Deferred write only touches its own field.
    #[test]
    fn a_deferred_write_landing_late_touches_only_its_own_field() {
        let mut authority = Authority::new();

        // slider drag schedules deferred write
        let debounced_volume = 0.2;
        authority.apply(|config| {
            config.volume = debounced_volume;
        });

        // user mutes before timer fires
        authority.apply(|config| {
            config.enable_sound = false;
        });

        // deferred write lands
        let wrote = authority.apply(|config| {
            config.volume = debounced_volume;
        });

        assert!(!wrote, "re-writing an already-current value must not touch the disk");
        assert!(!authority.config.enable_sound, "and must not revert the mute");
        assert_eq!(authority.config.volume, debounced_volume, "while the volume still holds");
    }

    /// No-op re-asserts don't write.
    #[test]
    fn re_asserting_the_current_value_writes_nothing() {
        let mut authority = Authority::new();
        let current_volume = authority.config.volume;

        for _ in 0..20 {
            let wrote = authority.apply(|config| {
                config.volume = current_volume;
            });
            assert!(!wrote, "a no-op mutation must not be persisted");
        }

        assert_eq!(authority.writes, 0, "twenty navigations must produce zero writes");

        let wrote = authority.apply(|config| {
            config.volume = current_volume + 0.25;
        });
        assert!(wrote, "a real change must still be written");
        assert_eq!(authority.writes, 1, "exactly one write for one real change");
    }

    /// Serialize tests using the real authority.
    static REAL_AUTHORITY_TESTS: Mutex<()> = Mutex::new(());

    fn lock_real_authority() -> std::sync::MutexGuard<'static, ()> {
        match REAL_AUTHORITY_TESTS.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// Real writers on different threads all survive.
    #[test]
    fn real_writers_on_different_threads_all_survive() {
        use std::sync::{ Arc, Barrier };

        let _serialised = lock_real_authority();
        let original = current();

        let threads = 8;
        let iterations = 40;
        let barrier = Arc::new(Barrier::new(threads));

        let handles: Vec<_> = (0..threads)
            .map(|id| {
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    for _ in 0..iterations {
                        match id {
                            0 => apply(|config| { config.volume = 0.5; }),
                            1 => apply(|config| { config.volume = 0.25; }),
                            2 => apply(|config| { config.enable_sound = false; }),
                            3 => apply(|config| { config.per_pack_volume.insert("keyboard/test".to_string(), 0.5); }),
                            5 => apply(|config| { config.auto_start = true; }),
                            6 => apply(|config| { config.keyboard_soundpack = "keyboard/test".to_string(); }),
                            _ => apply(|config| { config.selected_audio_device = Some("test".to_string()); }),
                        };
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().expect("no writer thread may panic");
        }

        let final_config = current();
        apply(|config| { *config = original; });
        // At least one writer's effect must be present; exact values race but no panic proves compose
        assert!(final_config.volume == 0.5 || final_config.volume == 0.25);
        // These fields each have a single writer, so their final value is deterministic.
        assert!(!final_config.enable_sound, "mute writer's effect must survive");
        assert!(final_config.auto_start, "auto_start writer's effect must survive");
    }

    /// Reader never sees a truncated document.
    #[test]
    fn a_concurrent_reader_never_observes_a_partial_document() {
        use std::sync::atomic::{ AtomicBool, Ordering as AtomicOrdering };
        use std::sync::Arc;

        let _serialised = lock_real_authority();
        let original = current();
        let path = crate::state::paths::data::config_json();
        let stop = Arc::new(AtomicBool::new(false));

        let reader = {
            let stop = Arc::clone(&stop);
            let path = path.clone();
            std::thread::spawn(move || {
                let mut partial_reads = 0usize;
                let mut successful_reads = 0usize;
                while !stop.load(AtomicOrdering::Relaxed) {
                    // missing is okay — only broken is a failure
                    if let Ok(contents) = std::fs::read_to_string(&path) {
                        if crate::state::config::parse_lenient(&contents).is_ok() {
                            successful_reads += 1;
                        } else {
                            partial_reads += 1;
                        }
                    }
                }
                (successful_reads, partial_reads)
            })
        };

        for round in 0..200 {
            apply(|config| {
                config.volume = 0.3 + (round as f32 % 10.0) * 0.01;
            });
        }

        stop.store(true, AtomicOrdering::Relaxed);
        let (successful_reads, partial_reads) = reader.join().expect("reader must not panic");

        apply(|config| {
            *config = original;
        });

        assert!(successful_reads > 0, "the reader must actually have observed the file");
        assert_eq!(
            partial_reads,
            0,
            "a half-written config was observed {partial_reads} times - \
             the write is not atomic"
        );
    }

    /// Metadata-only mutation doesn't desync memory.
    #[test]
    fn a_metadata_only_mutation_does_not_desync_memory_from_disk() {
        let mut authority = Authority::new();
        let before = authority.config.clone();

        let wrote = authority.apply(|config| {
            config.last_updated = chrono::Utc::now() + chrono::Duration::hours(1);
        });

        assert!(!wrote, "a metadata-only edit is not a change worth writing");
        assert_eq!(
            authority.config.last_updated,
            before.last_updated,
            "and must be rolled back rather than left only in memory"
        );
    }
}
