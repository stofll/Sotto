//! Duck the system output volume while recording.
//!
//! Two reasons, and the second is the one that matters for other people's
//! machines: whatever is playing leaks into the microphone, and you cannot
//! hear yourself over it. On a desktop with a headset neither is a problem;
//! on a laptop with built-in speakers both are.
//!
//! Off by default. Moving somebody's volume slider without being asked is
//! not a reasonable default, however well-intentioned.
//!
//! Windows-only. Every entry point is a no-op elsewhere.

use serde_json::Value;

/// Config key: duck the output while recording.
const CONFIG_ENABLED: &str = "duck_output_while_recording";
/// Config key: what to duck *to*, as a fraction of full scale.
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

/// Вне Windows `submit` — пустышка, и поля запроса никто не читает. Это не
/// мёртвый код, а форма «каждая точка входа здесь — no-op»: `duck` и
/// `restore` собирают запрос одинаково на всех платформах, чтобы ветвление
/// по ОС жило в одном месте — в `submit`.
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
fn worker() -> &'static std::sync::mpsc::SyncSender<Request> {
    use std::sync::mpsc::{sync_channel, SyncSender};
    use std::sync::OnceLock;

    static WORKER: OnceLock<SyncSender<Request>> = OnceLock::new();

    WORKER.get_or_init(|| {
        let (tx, rx) = sync_channel::<Request>(4);
        std::thread::Builder::new()
            .name("output-volume".to_string())
            .spawn(move || {
                unsafe { windows_impl::init_com() };
                // What the volume was before we touched it. `None` means we
                // are not currently ducked.
                let mut previous = None;
                while let Ok(request) = rx.recv() {
                    let (outcome, reply) = match request {
                        Request::Duck { level, reply } => (
                            unsafe { windows_impl::duck(&mut previous, level) }
                                .map_err(|error| error.to_string()),
                            reply,
                        ),
                        Request::Restore { reply } => (
                            unsafe { windows_impl::restore(&mut previous) }
                                .map_err(|error| error.to_string()),
                            reply,
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
    use windows::core::Result;
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{
        eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    /// # Safety
    /// Must run on the volume worker thread, once, before any other call
    /// here. A failure is not fatal: another component may have already put
    /// this thread in an apartment, and the volume calls work regardless.
    pub unsafe fn init_com() {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }

    /// Open the default playback endpoint's volume control.
    ///
    /// Resolved per call rather than cached: the default device changes when
    /// headphones are plugged in, and a cached handle would then be adjusting
    /// the volume of a device nobody is listening to.
    unsafe fn endpoint() -> Result<IAudioEndpointVolume> {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        // Browser, music and video playback use the multimedia role. The
        // previous eConsole selection can point at a different Windows 11
        // endpoint (for example HDMI vs headphones), making ducking appear
        // to do nothing even though another device's slider moved.
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
        device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
    }

    pub struct DuckState {
        endpoint: IAudioEndpointVolume,
        level: f32,
    }

    /// # Safety
    /// Calls into COM; see [`init_com`].
    pub unsafe fn duck(previous: &mut Option<DuckState>, level: f32) -> Result<()> {
        if previous.is_some() {
            return Ok(()); // already ducked
        }
        let volume = endpoint()?;
        let current = volume.GetMasterVolumeLevelScalar()?;
        // Nothing to duck to if the user is already quieter than the target.
        if current <= level {
            log::debug!("output volume already at {current:.3}, target {level:.3}");
            return Ok(());
        }
        volume.SetMasterVolumeLevelScalar(level, std::ptr::null())?;
        log::info!("output volume ducked {current:.3} → {level:.3}");
        *previous = Some(DuckState {
            endpoint: volume,
            level: current,
        });
        Ok(())
    }

    /// # Safety
    /// Calls into COM; see [`init_com`].
    pub unsafe fn restore(previous: &mut Option<DuckState>) -> Result<()> {
        let Some(state) = previous.as_ref() else {
            return Ok(());
        };
        // Restore the exact endpoint that was changed. The default output can
        // switch during a recording when headphones are connected.
        state
            .endpoint
            .SetMasterVolumeLevelScalar(state.level, std::ptr::null())?;
        log::info!("output volume restored to {:.3}", state.level);
        *previous = None;
        Ok(())
    }
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

    /// Touches the real audio endpoint:
    /// `cargo test --lib output_volume::tests::round_trip -- --ignored --nocapture`
    ///
    /// Ignored because it moves the machine's volume slider. Worth having
    /// anyway — nothing short of a real COM call proves the interface is
    /// being driven correctly, and the failure mode of getting it wrong is
    /// "the volume silently never changes".
    #[cfg(windows)]
    #[test]
    #[ignore = "changes the system output volume"]
    fn round_trip() {
        unsafe {
            windows_impl::init_com();
            let before = read_master_volume().expect("read volume");
            let mut previous = None;
            windows_impl::duck(&mut previous, 0.05).expect("duck");
            let ducked = read_master_volume().expect("read volume");
            windows_impl::restore(&mut previous).expect("restore");
            let after = read_master_volume().expect("read volume");
            println!("before={before:.3} ducked={ducked:.3} after={after:.3}");
            assert!(ducked < before, "volume did not drop");
            assert!((after - before).abs() < 0.01, "volume was not restored");
            assert!(previous.is_none(), "restore must clear the saved level");
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
