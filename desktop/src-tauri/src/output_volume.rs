//! Duck other applications' audio while recording.
//!
//! Two reasons, and the second is the one that matters for other people's
//! machines: whatever is playing leaks into the microphone, and you cannot
//! hear yourself over it. On a desktop with a headset neither is a problem;
//! on a laptop with built-in speakers both are.
//!
//! Per-application sessions rather than the endpoint's master volume: the
//! master slider also scales our own start/stop cues, which play while the
//! duck is being applied and lifted, so they came out muffled.
//!
//! Off by default. Moving somebody's volume slider without being asked is
//! not a reasonable default, however well-intentioned.
//!
//! Windows-only. Every entry point is a no-op elsewhere.

use serde_json::Value;

/// Config key: duck the output while recording.
const CONFIG_ENABLED: &str = "duck_output_while_recording";
/// Config key: what to duck *to*, as a fraction of each application's
/// current volume.
const CONFIG_LEVEL: &str = "duck_output_level";
/// Quiet enough to stop bleed into the microphone, loud enough that the
/// user can still tell something is playing.
const DEFAULT_LEVEL: f32 = 0.2;

/// Resolve the ducking target, or `None` when the feature is off.
fn target_level(config: &Value) -> Option<f32> {
    let enabled = config
        .get(CONFIG_ENABLED)
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !enabled {
        return None;
    }
    let level = config
        .get(CONFIG_LEVEL)
        .and_then(Value::as_f64)
        .map(|value| value as f32)
        .unwrap_or(DEFAULT_LEVEL);
    Some(level.clamp(0.0, 1.0))
}

/// Lower the output volume, remembering what it was.
///
/// Idempotent: a second call while already ducked does not overwrite the
/// remembered level, so a stray duck cannot make [`restore`] set the volume
/// to the ducked value permanently.
pub fn duck(config: &Value) {
    let Some(level) = target_level(config) else {
        return;
    };
    submit(Request::Duck { level, reply: None });
}

/// Put the volume back where it was. Safe to call when not ducked.
///
/// Must be reachable from every path that ends a recording — stop, cancel,
/// and the error branches. A recording that ends without this leaves the
/// machine quiet with no indication why.
pub fn restore() {
    submit(Request::Restore { reply: None });
}

/// [`restore`] that waits for the worker, for the exit path: a queued restore
/// would never run once the process is gone.
pub fn restore_before_exit() {
    #[cfg(windows)]
    if WORKER.get().is_some() {
        if let Err(error) = request_with_reply(|reply| Request::Restore { reply: Some(reply) }) {
            log::warn!("output volume: restore on exit: {error}");
        }
    }
}

/// Put back what an earlier run ducked and never restored — it crashed or was
/// killed mid-recording. Windows keeps an application's volume across its
/// restarts, so without this the other apps would stay quiet for good.
pub fn recover() {
    if journal_path().exists() {
        submit(Request::Recover);
    }
}

/// The ducked sessions, kept on disk from the duck until its restore. An
/// entry whose application is not running at recovery waits here for it.
const JOURNAL_FILE: &str = "output-duck.json";
/// How long an entry waits for its application before it is dropped: the
/// identifiers name other programs' executables, so they must not pile up.
const JOURNAL_MAX_AGE_SECS: u64 = 7 * 24 * 60 * 60;

fn journal_path() -> std::path::PathBuf {
    crate::user_data::data_dir().join(JOURNAL_FILE)
}

/// One ducked session, by the identifier Windows keeps its volume under — it
/// survives the application's restart, unlike the session instance.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct Ducked {
    session: String,
    original: f32,
    ducked: f32,
    /// When the duck happened, in seconds since the Unix epoch.
    #[serde(default = "now_secs")]
    at: u64,
}

#[cfg_attr(not(windows), allow(dead_code))]
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// The entries still owed a restore: neither handled in this pass nor too old.
#[cfg_attr(not(windows), allow(dead_code))]
fn still_waiting(entries: Vec<Ducked>, handled: &[String], now: u64) -> Vec<Ducked> {
    entries
        .into_iter()
        .filter(|entry| {
            !handled.contains(&entry.session) && now.saturating_sub(entry.at) < JOURNAL_MAX_AGE_SECS
        })
        .collect()
}

/// Keep what is still waiting, or remove the journal once nothing is.
#[cfg_attr(not(windows), allow(dead_code))]
fn save_waiting(path: &std::path::Path, entries: &[Ducked]) {
    if entries.is_empty() {
        let _ = std::fs::remove_file(path);
    } else if let Err(error) = write_journal(path, entries) {
        log::warn!("output volume: journal not written: {error}");
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn write_journal(path: &std::path::Path, entries: &[Ducked]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let json = serde_json::to_vec(entries).map_err(|error| error.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|error| error.to_string())?;
    std::fs::rename(&tmp, path).map_err(|error| error.to_string())
}

#[cfg_attr(not(windows), allow(dead_code))]
fn read_journal(path: &std::path::Path) -> Vec<Ducked> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Whether a session still sits where the duck left it. Any other level is a
/// change made since — by the user or the app itself — and stays.
#[cfg_attr(not(windows), allow(dead_code))]
fn still_ducked(current: f32, ducked: f32) -> bool {
    (current - ducked).abs() < 0.005
}

/// Temporarily duck the output and report backend errors to the caller.
/// Used by the settings-page check; unlike normal recording this waits for
/// the worker so a broken endpoint is visible instead of being only a log.
pub fn preview(level: f32, duration: std::time::Duration) -> Result<(), String> {
    #[cfg(windows)]
    {
        let ducked = request_with_reply(|reply| Request::Duck {
            level: level.clamp(0.0, 1.0),
            reply: Some(reply),
        })?;
        std::thread::sleep(duration);
        let restored = request_with_reply(|reply| Request::Restore { reply: Some(reply) });
        ducked?;
        restored?
    }
    #[cfg(not(windows))]
    {
        let _ = (level, duration);
        Err("output ducking is only implemented on Windows".to_string())
    }
}

/// Outside Windows `submit` is a no-op and nobody reads the request fields.
/// This is not dead code but the shape "every entry point here is a no-op":
/// `duck` and `restore` build the request identically on every platform so
/// that the OS branching lives in one place — inside `submit`.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug)]
enum Request {
    Duck {
        level: f32,
        reply: Option<std::sync::mpsc::Sender<Result<(), String>>>,
    },
    Restore {
        reply: Option<std::sync::mpsc::Sender<Result<(), String>>>,
    },
    Recover,
}

/// Run volume changes on one dedicated thread.
///
/// Two reasons for a worker rather than inline calls. COM apartments: the
/// main thread is STA (Tauri initialises it), and this wants MTA, which can
/// only be chosen once per thread. Ordering: duck and restore come from
/// different threads — the hotkey handler and the audio worker — and a
/// restore overtaking its duck would leave the volume down for good.
#[cfg(windows)]
fn submit(request: Request) {
    let tx = worker();
    // Dropping a duck is harmless. Dropping a restore is not — but the queue
    // only fills if the worker is wedged, and then a blocking send would
    // wedge the caller too.
    if tx.try_send(request).is_err() {
        log::warn!("output volume: request dropped, queue full");
    }
}

#[cfg(windows)]
static WORKER: std::sync::OnceLock<std::sync::mpsc::SyncSender<Request>> =
    std::sync::OnceLock::new();

#[cfg(windows)]
fn worker() -> &'static std::sync::mpsc::SyncSender<Request> {
    use std::sync::mpsc::sync_channel;

    WORKER.get_or_init(|| {
        let (tx, rx) = sync_channel::<Request>(4);
        std::thread::Builder::new()
            .name("output-volume".to_string())
            .spawn(move || {
                // SAFETY: this is the volume worker thread, and this is
                // its first statement — the apartment is initialised once,
                // before any of the COM calls below.
                unsafe { windows_impl::init_com() };
                // What the volume was before we touched it. `None` means we
                // are not currently ducked.
                let mut previous = None;
                let journal = journal_path();
                while let Ok(request) = rx.recv() {
                    let (outcome, reply) = match request {
                        Request::Duck { level, reply } => (
                            // SAFETY: same thread as the `init_com` above.
                            unsafe { windows_impl::duck(&mut previous, level, &journal) }
                                .map_err(|error| error.to_string()),
                            reply,
                        ),
                        Request::Restore { reply } => (
                            // SAFETY: same thread as the `init_com` above.
                            unsafe { windows_impl::restore(&mut previous, &journal) }
                                .map_err(|error| error.to_string()),
                            reply,
                        ),
                        Request::Recover => (
                            // SAFETY: same thread as the `init_com` above.
                            unsafe { windows_impl::recover(&journal) }
                                .map_err(|error| error.to_string()),
                            None,
                        ),
                    };
                    if let Err(error) = &outcome {
                        log::warn!("output volume: {error}");
                    }
                    if let Some(reply) = reply {
                        let _ = reply.send(outcome);
                    }
                }
            })
            .ok();
        tx
    })
}

#[cfg(not(windows))]
fn submit(_request: Request) {}

#[cfg(windows)]
fn request_with_reply(
    make: impl FnOnce(std::sync::mpsc::Sender<Result<(), String>>) -> Request,
) -> Result<Result<(), String>, String> {
    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
    worker()
        .try_send(make(reply_tx))
        .map_err(|error| format!("output volume worker unavailable: {error}"))?;
    reply_rx
        .recv_timeout(std::time::Duration::from_secs(3))
        .map_err(|_| "output volume worker did not respond".to_string())
}

#[cfg(windows)]
mod windows_impl {
    use windows::core::{Interface, Result};
    use windows::Win32::Media::Audio::{
        eMultimedia, eRender, AudioSessionStateExpired, IAudioSessionControl2,
        IAudioSessionManager2, IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    use super::{
        now_secs, read_journal, save_waiting, still_ducked, still_waiting, write_journal, Ducked,
    };
    use std::path::Path;

    /// # Safety
    /// Must run on the volume worker thread, once, before any other call
    /// here. A failure is not fatal: another component may have already put
    /// this thread in an apartment, and the volume calls work regardless.
    pub unsafe fn init_com() {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }

    /// Open the default playback endpoint's session manager.
    ///
    /// Resolved per call rather than cached: the default device changes when
    /// headphones are plugged in, and a cached handle would then be adjusting
    /// the volume of a device nobody is listening to.
    unsafe fn session_manager() -> Result<IAudioSessionManager2> {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        // Browser, music and video playback use the multimedia role. The
        // previous eConsole selection can point at a different Windows 11
        // endpoint (for example HDMI vs headphones), making ducking appear
        // to do nothing even though another device's slider moved.
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
        device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None)
    }

    /// The identifier Windows keeps a session's volume under, if it reads.
    unsafe fn session_identifier(control: &IAudioSessionControl2) -> Option<String> {
        let id = control.GetSessionIdentifier().ok()?;
        let text = id.to_string().ok();
        CoTaskMemFree(Some(id.0 as *const _));
        text
    }

    /// The live sessions of other processes, with their identifiers. Our own
    /// is left out so the cues keep their volume whenever they play relative
    /// to the duck.
    unsafe fn foreign_sessions() -> Result<Vec<(ISimpleAudioVolume, Option<String>)>> {
        let sessions = session_manager()?.GetSessionEnumerator()?;
        let own_pid = std::process::id();
        let mut out = Vec::new();
        for index in 0..sessions.GetCount()? {
            let Ok(control) = sessions.GetSession(index) else {
                continue;
            };
            if control
                .GetState()
                .is_ok_and(|state| state == AudioSessionStateExpired)
            {
                continue;
            }
            let Ok(control2) = control.cast::<IAudioSessionControl2>() else {
                continue;
            };
            if control2.GetProcessId().is_ok_and(|pid| pid == own_pid) {
                continue;
            }
            if let Ok(volume) = control.cast::<ISimpleAudioVolume>() {
                out.push((volume, session_identifier(&control2)));
            }
        }
        Ok(out)
    }

    /// Each ducked session with the volume to put back, and the identifiers
    /// of those the journal holds.
    pub struct DuckState(Vec<(ISimpleAudioVolume, f32)>, Vec<String>);

    /// # Safety
    /// Must run on a thread that has entered the same COM apartment
    /// [`init_com`] created — in practice the single volume worker, which is
    /// the only place this is called from. The interfaces stored in
    /// `previous` belong to that apartment and cannot be used outside it.
    pub unsafe fn duck(previous: &mut Option<DuckState>, level: f32, journal: &Path) -> Result<()> {
        if previous.is_some() {
            return Ok(()); // already ducked
        }
        // A session an earlier run left ducked comes back first, or its
        // lowered level would be remembered as the one to restore.
        if journal.exists() {
            if let Err(error) = recover(journal) {
                log::warn!("output volume: recovery before duck: {error}");
            }
        }
        let mut ducked = Vec::new();
        let mut sessions = Vec::new();
        // Entries still waiting for their application stay in the journal.
        let mut entries = read_journal(journal);
        let at = now_secs();
        // Per-session failures are skipped rather than returned: a session
        // already lowered must still be remembered, or it would stay quiet.
        for (volume, session) in foreign_sessions()? {
            let Ok(current) = volume.GetMasterVolume() else {
                continue;
            };
            let lowered = current * level;
            if volume.SetMasterVolume(lowered, std::ptr::null()).is_ok() {
                if let Some(session) = session {
                    sessions.push(session.clone());
                    entries.push(Ducked {
                        session,
                        original: current,
                        ducked: lowered,
                        at,
                    });
                }
                ducked.push((volume, current));
            }
        }
        log::info!(
            "output volume: ducked {} session(s) to {level:.3}",
            ducked.len()
        );
        if !entries.is_empty() {
            if let Err(error) = write_journal(journal, &entries) {
                log::warn!("output volume: journal not written: {error}");
            }
        }
        if !ducked.is_empty() {
            *previous = Some(DuckState(ducked, sessions));
        }
        Ok(())
    }

    /// # Safety
    /// Must run in the same COM apartment as the [`duck`] call that filled
    /// `previous`: the session interfaces inside it cannot cross apartments.
    pub unsafe fn restore(previous: &mut Option<DuckState>, journal: &Path) -> Result<()> {
        let Some(DuckState(ducked, sessions)) = previous.take() else {
            return Ok(());
        };
        // The exact sessions that were changed, even if the default output
        // switched during the recording. One whose app closed meanwhile fails
        // harmlessly and must not stop the rest from coming back.
        for (volume, level) in &ducked {
            if let Err(error) = volume.SetMasterVolume(*level, std::ptr::null()) {
                log::debug!("output volume: session not restored: {error}");
            }
        }
        save_waiting(
            journal,
            &still_waiting(read_journal(journal), &sessions, now_secs()),
        );
        log::info!("output volume: restored {} session(s)", ducked.len());
        Ok(())
    }

    /// Restore the sessions an earlier run left ducked, from its journal.
    /// One whose application is not playing now stays in the journal for a
    /// later start or duck, until it ages out.
    ///
    /// # Safety
    /// Same apartment rule as [`duck`].
    pub unsafe fn recover(journal: &Path) -> Result<()> {
        let entries = read_journal(journal);
        let mut restored = 0;
        let mut handled = Vec::new();
        if !entries.is_empty() {
            for (volume, session) in foreign_sessions()? {
                let Some(entry) = session
                    .and_then(|session| entries.iter().find(|entry| entry.session == session))
                else {
                    continue;
                };
                let Ok(current) = volume.GetMasterVolume() else {
                    continue;
                };
                // Found either way: a level changed since the duck stays.
                handled.push(entry.session.clone());
                if still_ducked(current, entry.ducked)
                    && volume
                        .SetMasterVolume(entry.original, std::ptr::null())
                        .is_ok()
                {
                    restored += 1;
                }
            }
        }
        let waiting = still_waiting(entries, &handled, now_secs());
        save_waiting(journal, &waiting);
        log::info!(
            "output volume: recovered {restored} session(s) left ducked by an earlier run, {} waiting",
            waiting.len()
        );
        Ok(())
    }
}

/// Temporarily lower other applications' audio so the setting can be
/// verified without starting a recording. Unlike normal best-effort ducking,
/// this command returns the Core Audio error to the settings UI.
#[tauri::command]
pub(crate) async fn preview_output_duck(level: f64) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        preview(level as f32, std::time::Duration::from_millis(1200))
    })
    .await
    .map_err(|error| format!("output volume preview task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ducking_is_off_by_default() {
        assert_eq!(target_level(&json!({})), None);
    }

    #[test]
    fn enabling_uses_the_default_level() {
        assert_eq!(
            target_level(&json!({ "duck_output_while_recording": true })),
            Some(DEFAULT_LEVEL)
        );
    }

    #[test]
    fn level_is_read_and_clamped() {
        assert_eq!(
            target_level(&json!({
                "duck_output_while_recording": true,
                "duck_output_level": 0.5,
            })),
            Some(0.5)
        );
        // A hand-edited config must not be able to ask for a volume outside
        // the scalar range — SetMasterVolumeLevelScalar would just fail and
        // the recording would play at full blast.
        assert_eq!(
            target_level(&json!({
                "duck_output_while_recording": true,
                "duck_output_level": 9.0,
            })),
            Some(1.0)
        );
        assert_eq!(
            target_level(&json!({
                "duck_output_while_recording": true,
                "duck_output_level": -1.0,
            })),
            Some(0.0)
        );
    }

    #[test]
    fn restore_without_duck_is_harmless() {
        // Called on every stop path, including ones where ducking was off.
        restore();
    }

    #[test]
    fn the_journal_round_trips_and_a_missing_one_reads_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join(JOURNAL_FILE);
        assert!(read_journal(&path).is_empty());
        let entries = vec![Ducked {
            session: "{0.0.0.00000000}.{guid}|player.exe".into(),
            original: 0.8,
            ducked: 0.16,
            at: 1_700_000_000,
        }];
        write_journal(&path, &entries).unwrap();
        assert_eq!(read_journal(&path), entries);
        std::fs::write(&path, b"not json").unwrap();
        assert!(read_journal(&path).is_empty());
    }

    #[test]
    fn an_entry_waits_for_its_application_until_it_ages_out() {
        let entry = |session: &str, at: u64| Ducked {
            session: session.into(),
            original: 0.8,
            ducked: 0.16,
            at,
        };
        let now = 1_700_000_000;
        let waiting = still_waiting(
            vec![
                entry("handled", now),
                entry("closed", now - 60),
                entry("stale", now - JOURNAL_MAX_AGE_SECS),
            ],
            &["handled".to_string()],
            now,
        );
        assert_eq!(waiting, vec![entry("closed", now - 60)]);
    }

    #[test]
    fn a_journal_without_timestamps_waits_from_now() {
        let entries: Vec<Ducked> =
            serde_json::from_str(r#"[{"session":"s","original":0.8,"ducked":0.16}]"#).unwrap();
        assert_eq!(still_waiting(entries, &[], now_secs()).len(), 1);
    }

    #[test]
    fn save_waiting_removes_an_empty_journal_and_keeps_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(JOURNAL_FILE);
        let entries = vec![Ducked {
            session: "s".into(),
            original: 0.8,
            ducked: 0.16,
            at: now_secs(),
        }];
        save_waiting(&path, &entries);
        assert_eq!(read_journal(&path), entries);
        save_waiting(&path, &[]);
        assert!(!path.exists());
    }

    #[test]
    fn recovery_leaves_a_volume_changed_since_the_duck() {
        assert!(still_ducked(0.16, 0.16));
        assert!(still_ducked(0.162, 0.16));
        assert!(!still_ducked(0.5, 0.16));
    }

    /// Touches the real audio endpoint:
    /// `cargo test --lib output_volume::tests::round_trip -- --ignored --nocapture`
    ///
    /// Ignored because it moves other applications' volume; start some
    /// playback first, or there is nothing to duck. Worth having anyway —
    /// nothing short of a real COM call proves the interfaces are being
    /// driven correctly.
    #[cfg(windows)]
    #[test]
    #[ignore = "changes other applications' output volume"]
    fn round_trip() {
        // SAFETY: the test body is the whole life of this thread, so
        // `init_com` runs first and every COM call below stays on it.
        unsafe {
            windows_impl::init_com();
            let before = read_master_volume().expect("read volume");
            let dir = tempfile::tempdir().unwrap();
            let journal = dir.path().join(JOURNAL_FILE);
            let mut previous = None;
            windows_impl::duck(&mut previous, 0.05, &journal).expect("duck");
            let during = read_master_volume().expect("read volume");
            println!("ducked sessions: {}", previous.is_some());
            windows_impl::restore(&mut previous, &journal).expect("restore");
            assert!(!journal.exists(), "restore must remove the journal");
            // The master slider scales our own cues too, so it must not move.
            assert!((during - before).abs() < 0.01, "master volume changed");
            assert!(previous.is_none(), "restore must clear the saved levels");
        }
    }

    #[cfg(windows)]
    unsafe fn read_master_volume() -> windows::core::Result<f32> {
        use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
        use windows::Win32::Media::Audio::{
            eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
        };
        use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
        device
            .Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)?
            .GetMasterVolumeLevelScalar()
    }
}
