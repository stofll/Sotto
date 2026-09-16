//! Mutex-poison recovery (Phase 4 / Batch 6 / P0).
//!
//! Wrapper around `std::sync::Mutex::lock()` that recovers from a
//! poisoned mutex instead of panicking. A poisoned mutex means a
//! previous lock-holder panicked while holding the lock. For most
//! of our state (FSM, model registry, cancel-flag map), a poisoned
//! lock does NOT mean the state is corrupted — it just means a
//! concurrent worker died and we want to drive the FSM forward
//! into a sane state anyway. Cascading panics from a worker
//! crash used to take down the whole Tauri shell; this helper
//! ends the cascade at the first poisoned mutex.
//!
//! The same applies to the SQLite connection behind stats,
//! history and telemetry (one shared `Mutex<Connection>`) and to
//! the file-logger state: a panic under any of those locks would
//! otherwise make every later write — or every later `log::` call
//! — panic in turn.
//!
//! Audio-thread locks (cpal callback in `audio.rs`) intentionally
//! continue to use `.lock().unwrap()` — a poisoned mutex on the
//! audio thread IS a fatal bug, and panicking is the right
//! behavior so the OS/host can dump a core.
//!
//! Usage: `let mut guard = mutex_recover::lock(&state.app_fsm);`.

use std::collections::BTreeSet;
use std::sync::{Mutex, MutexGuard};

/// Report recovery once per type: poison persists, and the logger takes
/// its state lock on every record.
static REPORTED: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());

/// `true` the first time a given type recovers, `false` afterwards.
fn should_report(type_name: &'static str) -> bool {
    REPORTED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(type_name)
}

/// Lock a mutex, recovering from poisoning by returning the
/// inner data anyway. Use this for FSM / registry / dispatcher
/// state where a previous panic shouldn't cascade into a second
/// panic from us. Returns a `MutexGuard` with the same lifetime
/// as the inner data.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            // Deliberately `eprintln!` and not `log::warn!`: the file
            // logger's own state lives behind one of the mutexes this
            // helper recovers, and `BridgeLogger::log` would re-enter it.
            // `std::sync::Mutex` is not reentrant, so reporting through
            // `log` would deadlock on exactly the case worth reporting.
            let type_name = std::any::type_name::<T>();
            if should_report(type_name) {
                eprintln!("[mutex_recover] recovered from poisoned Mutex at {type_name}");
            }
            poisoned.into_inner()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn healthy_mutex_returns_normal_guard() {
        let mutex: Mutex<u32> = Mutex::new(0);
        *lock(&mutex) = 42;
        assert_eq!(*lock(&mutex), 42);
    }

    #[test]
    fn poisoned_mutex_recovers_instead_of_panicking() {
        let mutex: std::sync::Arc<Mutex<String>> =
            std::sync::Arc::new(Mutex::new(String::from("live")));
        // Poison the mutex by panicking while holding the lock.
        let mutex_clone = std::sync::Arc::clone(&mutex);
        let join_result = std::thread::spawn(move || {
            let mut guard = mutex_clone.lock().unwrap();
            guard.push_str("-poisoned");
            panic!("simulated worker panic");
        })
        .join();
        assert!(join_result.is_err(), "worker thread should panic");
        assert!(mutex.is_poisoned(), "mutex must be poisoned");
        // Recovery must yield the inner data and let us mutate
        // instead of cascading the panic. The lock helper
        // returns the inner MutexGuard from inside the
        // PoisonError so the data is still editable.
        let recovered = lock(&mutex);
        assert!(
            recovered.starts_with("live-poisoned"),
            "expected pre-panic data, got: {recovered}"
        );
    }

    #[test]
    fn a_recovered_type_is_reported_once() {
        // Names local to this test so a real recovery elsewhere in the
        // process cannot decide the outcome.
        assert!(should_report("test::ReportedOnce"));
        assert!(!should_report("test::ReportedOnce"));
        assert!(!should_report("test::ReportedOnce"));
    }

    #[test]
    fn a_different_type_is_reported_separately() {
        assert!(should_report("test::FirstResource"));
        assert!(should_report("test::SecondResource"));
    }
}
