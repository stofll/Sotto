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
//! Audio-thread locks (cpal callback in `audio.rs`) intentionally
//! continue to use `.lock().unwrap()` — a poisoned mutex on the
//! audio thread IS a fatal bug, and panicking is the right
//! behavior so the OS/host can dump a core.
//!
//! Usage: `let mut guard = mutex_recover::lock(&state.app_fsm);`.

use std::sync::{Mutex, MutexGuard};

/// Lock a mutex, recovering from poisoning by returning the
/// inner data anyway. Use this for FSM / registry / dispatcher
/// state where a previous panic shouldn't cascade into a second
/// panic from us. Returns a `MutexGuard` with the same lifetime
/// as the inner data.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            eprintln!(
                "[mutex_recover] recovered from poisoned Mutex at {}",
                std::any::type_name::<T>()
            );
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
    fn helper_returns_normal_guard_for_healthy_mutex() {
        let mutex = Mutex::new(42_u32);
        let mut guard = lock(&mutex);
        *guard = 100;
        drop(guard);
        assert_eq!(*lock(&mutex), 100);
    }
}
