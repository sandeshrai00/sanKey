/// Logs to console and to the in-RAM ring buffer shown in Settings > Debug.
pub fn init_debug_logging() {
    crate::always_print!("🐛 Debug logging enabled");
}
pub fn is_debug_enabled() -> bool {
    true
}

/// Debug print — only when debug is enabled.
#[macro_export]
macro_rules! debug_print {
    ($($arg:tt)*) => {
        if $crate::utils::logger::is_debug_enabled() {
            // format once, then tee to console and buffer
            let line = format!($($arg)*);
            println!("{}", line);
            $crate::utils::log_buffer::push(&line);
        }
    };
}

/// Debug error print — only when debug is enabled.
#[macro_export]
macro_rules! debug_eprint {
    ($($arg:tt)*) => {
        if $crate::utils::logger::is_debug_enabled() {
            let line = format!($($arg)*);
            eprintln!("{}", line);
            $crate::utils::log_buffer::push(&line);
        }
    };
}

/// Always prints, regardless of debug state.
#[macro_export]
macro_rules! always_print {
    ($($arg:tt)*) => {
        {
            let line = format!($($arg)*);
            println!("{}", line);
            $crate::utils::log_buffer::push(&line);
        }
    };
}

/// Always prints to stderr, regardless of debug state.
#[macro_export]
macro_rules! always_eprint {
    ($($arg:tt)*) => {
        {
            let line = format!($($arg)*);
            eprintln!("{}", line);
            $crate::utils::log_buffer::push(&line);
        }
    };
}

#[cfg(test)]
mod tests {
    /// Buffer must receive logs — release builds have nowhere else to look.
    #[test]
    fn always_print_reaches_the_ring_buffer() {
        // serialized with other buffer tests to avoid eviction
        let _guard = crate::utils::log_buffer::buffer_test_guard();

        let before = crate::utils::log_buffer::generation();
        crate::always_print!("logger test marker {}", 42);
        assert!(
            crate::utils::log_buffer::generation() > before,
            "a logged line must land in the buffer"
        );

        let recent = crate::utils::log_buffer::recent(50).join("\n");
        assert!(recent.contains("logger test marker 42"), "{}", recent);
    }

    /// Arguments must be evaluated exactly once.
    #[test]
    fn arguments_are_evaluated_exactly_once() {
        let mut calls = 0;
        let mut bump = || {
            calls += 1;
            calls
        };
        crate::always_print!("evaluated {} time(s)", bump());
        assert_eq!(calls, 1, "a side-effecting argument must run once");
    }
}
