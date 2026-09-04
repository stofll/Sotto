use std::{
    sync::{
        mpsc::{channel, Sender},
        Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

use tauri::{
    command, AppHandle, Emitter, Listener, Manager, PhysicalPosition, Position, WebviewUrl,
    WebviewWindowBuilder,
};

#[cfg(windows)]
use super::windows::win_util::{
    apply_noactivate_styles, extract_hwnd, force_topmost_noactivate, hide_window, install_nc_guard,
    set_window_cloaked, show_window_noactivate,
};

const OVERLAY_LABEL: &str = "overlay";
const OVERLAY_WIDTH: f64 = 308.0;
const OVERLAY_HEIGHT: f64 = 64.0;
/// Размер окна под потоковую модель: живой текст не помещается в таблетку.
///
/// Окно, а не только плашка внутри него: у оверлея прозрачный фон, но клики
/// он всё равно перехватывает, поэтому держать его большим постоянно значит
/// накрыть невидимой полосой треть экрана. Растём только на время диктовки
/// потоковой моделью.
const OVERLAY_STREAM_WIDTH: f64 = 600.0;
const OVERLAY_STREAM_HEIGHT: f64 = 150.0;
// Where the window is born, before `position_overlay` places it. -32000 is
// the standard Win32 "minimized" sentinel, so nothing is visible even if
// something shows the window before we are ready for it. Not a parking spot
// to return to: a window that sits entirely outside every monitor gets
// classified as occluded and stops being composited.
const OFFSCREEN_X: i32 = -32000;
const OFFSCREEN_Y: i32 = -32000;

static LAST_STATE: Mutex<Option<String>> = Mutex::new(None);
static OVERLAY_READY: Mutex<bool> = Mutex::new(false);
static WINDOW_CREATE_LOCK: Mutex<()> = Mutex::new(());
/// Whether the pill is currently drawn.
///
/// This is the logical pill state rather than a direct visibility query. It
/// keeps duplicate state events from repeating reveal/z-order work and stays
/// consistent across the pre-warmed and hidden window paths.
static ON_SCREEN: Mutex<bool> = Mutex::new(false);

fn is_on_screen() -> bool {
    ON_SCREEN.lock().map(|g| *g).unwrap_or(false)
}

fn set_on_screen(value: bool) {
    if let Ok(mut guard) = ON_SCREEN.lock() {
        *guard = value;
    }
}

// ---------------------------------------------------------------------------
// Overlay worker thread.
//
// Every operation that touches the overlay window runs here, one at a time,
// and nothing else is allowed to touch it. The reason is a deadlock that
// froze the whole app:
//
//   * Tauri runs a non-`async` `#[command]` on the MAIN thread.
//   * `WebviewWindow::emit` / `show` / `set_position` / `is_visible` /
//     `primary_monitor` called from any other thread post a message to the
//     event loop and block until the MAIN thread answers.
//   * Engine events (`whisper-cancelled`, `recording-started`, …) are
//     delivered on a tokio worker.
//
// So the old code — a `WINDOW_VISIBILITY_LOCK` taken by both `hide()` on the
// main thread (React's `invoke("hide")`) and `hide()` on a tokio thread (the
// `whisper-cancelled` listener) — deadlocks the moment both fire together,
// which is exactly what cancelling does: the tokio side grabs the lock and
// then waits for the main thread, while the main thread waits for the lock.
// Windows then draws its unresponsive-window placeholder: a white box with
// the default frame, titled "Overlay".
//
// With a worker thread the commands only enqueue and return instantly, so
// the main thread never holds a lock the window code needs, and the worker
// is free to block on main-thread round-trips. The channel also preserves
// ordering, which the mutex never guaranteed.
// ---------------------------------------------------------------------------

enum OverlayOp {
    Show(String),
    Hide,
}

static OP_TX: OnceLock<Sender<OverlayOp>> = OnceLock::new();

fn post(op: OverlayOp) {
    match OP_TX.get() {
        Some(tx) => {
            if tx.send(op).is_err() {
                eprintln!("[overlay] worker thread is gone, dropping op");
            }
        }
        None => eprintln!("[overlay] worker thread not started yet, dropping op"),
    }
}

/// Spawn the overlay worker. Call once from `lib.rs::setup()`; later calls
/// are ignored.
pub fn start_worker(app: AppHandle) {
    let (tx, rx) = channel::<OverlayOp>();
    if OP_TX.set(tx).is_err() {
        return;
    }
    thread::spawn(move || {
        while let Ok(op) = rx.recv() {
            let result = match op {
                OverlayOp::Show(state) => apply_show(&app, state),
                OverlayOp::Hide => apply_hide(&app),
            };
            if let Err(e) = result {
                eprintln!("[overlay] worker op failed: {e}");
            }
        }
    });
}

/// Stop showing the pill.
///
/// The window used to stay visible and on-screen on Windows while React
/// rendered an empty transparent document. That avoided rebuilding the DWM
/// composition surface, but it also left the native HWND alive. If Windows
/// repainted or restored any non-client style, the empty WebView exposed a
/// permanent system frame titled "Overlay" until the application exited.
///
/// Actually hiding the window is the only reliable way to guarantee that no
/// native fallback surface can remain visible. `apply_show` primes React
/// while the window is hidden before `reveal` shows it again, which keeps the
/// WebView's next frame ready and limits the redirection-bitmap flash that the
/// old hide/show implementation suffered from.
fn conceal(window: &tauri::WebviewWindow) -> Result<(), String> {
    #[cfg(windows)]
    {
        let hwnd = extract_hwnd(window)?;
        // Issue #24: the reported flash happens here, on the first cancel.
        // Three snapshots bracket the two calls, so a caption that appears
        // between any two of them names the call that brought it back.
        crate::windows::overlay_diag::snapshot(hwnd, "conceal:enter");
        unsafe {
            // Cloak first: DWM stops presenting the HWND before ShowWindow
            // gets a chance to expose any intermediate native frame.
            if let Err(e) = set_window_cloaked(hwnd, true) {
                eprintln!("[overlay] cloak before hide failed (falling back to SW_HIDE): {e}");
            }
            crate::windows::overlay_diag::snapshot(hwnd, "conceal:after-cloak");
            hide_window(hwnd);
        }
        crate::windows::overlay_diag::snapshot(hwnd, "conceal:after-hide");
        // The enumeration is the half that can exonerate our HWND entirely:
        // a frame drawn by a window of another class is not ours to restyle.
        crate::windows::overlay_diag::enumerate_top_level(hwnd, "conceal:after-hide");
        Ok(())
    }
    #[cfg(not(windows))]
    {
        window.hide().map_err(|e| e.to_string())
    }
}

/// Start showing the pill. The Windows path performs no show and no move in
/// the steady state. After a conceal it shows the already-primed WebView,
/// reapplies the styles that tao may rebuild on a visibility transition,
/// then stops passing clicks through and re-asserts z-order.
fn reveal(window: &tauri::WebviewWindow) -> Result<(), String> {
    // A no-op unless the monitor layout changed under us, in which case the
    // window does move — one rare flash beats a permanently misplaced pill.
    position_overlay(window)?;
    #[cfg(windows)]
    {
        let hwnd = extract_hwnd(window)?;
        crate::windows::overlay_diag::snapshot(hwnd, "reveal:enter");
        unsafe {
            // Keep every intermediate frame behind DWM's cloak. This covers
            // the first ShowWindow/HideWindow cycle, where USER32 otherwise
            // briefly presents the native caption before settling.
            if let Err(e) = set_window_cloaked(hwnd, true) {
                eprintln!("[overlay] cloak before show failed (continuing): {e}");
            }
            // Before the restyle, so a caption present here proves something
            // between window creation and this point put it back — which is
            // the tao-rebuild hypothesis in one line.
            crate::windows::overlay_diag::snapshot(hwnd, "reveal:before-styles");
            apply_noactivate_styles(hwnd);
            crate::windows::overlay_diag::snapshot(hwnd, "reveal:after-styles");
            show_window_noactivate(hwnd);
            crate::windows::overlay_diag::snapshot(hwnd, "reveal:after-show");
            force_topmost_noactivate(hwnd);
            // Once ShowWindow marks the surface visible, give WebView2 a
            // frame to submit the already-queued React state before DWM is
            // allowed to present it.
            thread::sleep(Duration::from_millis(16));
            if let Err(e) = set_window_cloaked(hwnd, false) {
                eprintln!("[overlay] uncloak after show failed: {e}");
            }
        }
        crate::windows::overlay_diag::snapshot(hwnd, "reveal:after-uncloak");
        crate::windows::overlay_diag::enumerate_top_level(hwnd, "reveal:after-uncloak");
    }
    #[cfg(not(windows))]
    window.show().map_err(|e| e.to_string())?;
    Ok(())
}

fn set_last_state(state: Option<&str>) {
    if let Ok(mut guard) = LAST_STATE.lock() {
        *guard = state.map(|s| s.to_string());
    }
}

#[command]
pub fn current_state() -> Option<String> {
    LAST_STATE.lock().ok().and_then(|g| g.clone())
}

#[command]
pub fn overlay_ready() -> Result<(), String> {
    if let Ok(mut guard) = OVERLAY_READY.lock() {
        *guard = true;
    }
    Ok(())
}

fn wait_until_ready(timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if OVERLAY_READY.lock().map(|g| *g).unwrap_or(false) {
            return true;
        }
        thread::sleep(Duration::from_millis(8));
    }
    false
}

/// Build the overlay window if it doesn't exist yet, parked off-screen and hidden.
///
/// Called once at app startup. The transparent + decorations(false) combination
/// is the canonical cross-platform pattern (same as Handy, open-whisper) —
/// CSS inside React draws the pill shape with anti-aliasing, so we don't need
/// SetWindowRgn or any other Win32 chrome hacks. The single Win32-specific call
/// (`apply_noactivate_styles`) sets WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW so the
/// pill never steals focus from the target app.
///
/// # Deadlock prevention
///
/// `WINDOW_CREATE_LOCK` is acquired only for the double-check + ready-flag
/// reset, then released **before** `WebviewWindowBuilder::build()` (which
/// takes 200–500 ms to spawn a webview). Holding a global mutex across a
/// webview spawn is a deadlock smell — any future caller that takes a second
/// mutex in the same path would risk a cross-mutex deadlock.
#[cfg_attr(not(windows), allow(unused_variables))]
pub fn ensure_window(app: &AppHandle) -> Result<(), String> {
    if app.get_webview_window(OVERLAY_LABEL).is_some() {
        return Ok(());
    }

    // Scope: take the lock only for the double-check + ready-flag reset.
    // The lock must NOT be held across the heavy .build() call below.
    {
        let _guard = WINDOW_CREATE_LOCK
            .lock()
            .map_err(|_| "overlay window creation lock poisoned".to_string())?;
        if app.get_webview_window(OVERLAY_LABEL).is_some() {
            return Ok(());
        }

        if let Ok(mut guard) = OVERLAY_READY.lock() {
            *guard = false;
        }
    } // WINDOW_CREATE_LOCK released here

    let window =
        WebviewWindowBuilder::new(app, OVERLAY_LABEL, WebviewUrl::App("overlay.html".into()))
            .title("Overlay")
            .inner_size(OVERLAY_WIDTH, OVERLAY_HEIGHT)
            .position(OFFSCREEN_X as f64, OFFSCREEN_Y as f64)
            .resizable(false)
            .decorations(false)
            .shadow(false)
            .transparent(true)
            .always_on_top(true)
            .visible_on_all_workspaces(true)
            .skip_taskbar(true)
            .focused(false)
            .focusable(false)
            .visible(false)
            .build()
            .map_err(|e| e.to_string())?;

    #[cfg(windows)]
    {
        let hwnd = extract_hwnd(&window)?;
        crate::windows::overlay_diag::snapshot(hwnd, "ensure_window:after-build");
        unsafe {
            // Щит ставится до правки стилей, а не после: он должен стоять
            // раньше любого кадра, а не раньше любого стиля. Стили после
            // него всё равно применятся — они идут через SetWindowLongPtrW,
            // а не через оконную процедуру.
            if let Err(e) = install_nc_guard(hwnd) {
                eprintln!("[overlay] non-client guard not installed: {e}");
            }
            apply_noactivate_styles(hwnd);
        }
        // The baseline. Every later snapshot is read as a diff against this
        // one, so it has to be taken before anything else can touch the
        // window — including WebView2 finishing its own initialisation.
        crate::windows::overlay_diag::snapshot(hwnd, "ensure_window:after-styles");
    }
    #[cfg(target_os = "macos")]
    apply_macos_overlay_style(&window)?;
    Ok(())
}

/// macOS counterpart of `apply_noactivate_styles`: lift the NSWindow above
/// regular app windows and let it follow the user across Spaces and into
/// full-screen apps. `always_on_top` alone maps to NSFloatingWindowLevel,
/// which loses to full-screen windows and stays glued to the Space the
/// window was created on.
#[cfg(target_os = "macos")]
fn apply_macos_overlay_style(window: &tauri::WebviewWindow) -> Result<(), String> {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    let ns_window = window.ns_window().map_err(|e| e.to_string())? as usize;
    window
        .run_on_main_thread(move || {
            let ns = ns_window as *mut AnyObject;
            unsafe {
                // NSStatusWindowLevel (25): above normal + floating windows and
                // above full-screen app content, below screen savers/menus.
                let _: () = msg_send![ns, setLevel: 25isize];
                // CanJoinAllSpaces (1<<0) | IgnoresCycle (1<<6) |
                // FullScreenAuxiliary (1<<8): follow every Space including
                // full-screen ones, never participate in Cmd+` cycling.
                let behavior: usize = (1 << 0) | (1 << 6) | (1 << 8);
                let _: () = msg_send![ns, setCollectionBehavior: behavior];
            }
        })
        .map_err(|e| e.to_string())
}

/// Enqueue a state change. Returns as soon as the op is posted — see the
/// worker-thread comment for why this must never do the work inline.
#[command]
pub fn show_state(state: String, _app: AppHandle) -> Result<(), String> {
    post(OverlayOp::Show(state));
    Ok(())
}

/// Worker-thread body of `OverlayOp::Show`.
fn apply_show(app: &AppHandle, state: String) -> Result<(), String> {
    let prev_state = current_state();
    eprintln!("[overlay] apply_show({state}); current_state={prev_state:?}");
    set_last_state(Some(&state));
    ensure_window(app)?;
    let window = app
        .get_webview_window(OVERLAY_LABEL)
        .ok_or_else(|| "overlay window missing after ensure_window".to_string())?;
    wait_until_ready(Duration::from_millis(500));
    // Prime the hidden WebView before making its HWND visible. In particular,
    // after a real window.hide() this prevents show() from presenting the
    // previous transparent/default surface while React catches up.
    window
        .emit("overlay-state", &state)
        .map_err(|e| e.to_string())?;
    if !is_on_screen() {
        // Issue #24: number the show/hide cycles. The defect is reported on
        // the first cancel after launch, so every diagnostic line carries
        // the cycle it belongs to and cycle=1 is the only one that matters.
        #[cfg(windows)]
        {
            crate::windows::overlay_diag::begin_cycle();
            crate::windows::overlay_diag::probe_main_thread(app, "show:enter");
        }
        // `emit` schedules React's update. Give WebView2 one frame to paint
        // while the HWND is still hidden before reveal makes it visible.
        thread::sleep(Duration::from_millis(16));
        // Only on the hidden -> shown transition. Re-running this on every
        // state event is both pointless and harmful: `recording_stopped` and
        // `processing_started` arrive back-to-back and both land here.
        reveal(&window)?;
        set_on_screen(true);
        #[cfg(windows)]
        {
            // Re-assert z-order once the target app has settled — starting a
            // recording usually coincides with another window activating.
            let hwnd = extract_hwnd(&window)?;
            thread::sleep(Duration::from_millis(16));
            unsafe {
                force_topmost_noactivate(hwnd);
            }
        }
    }
    Ok(())
}

/// Pick the monitor the overlay should appear on.
///
/// The overlay is feedback about a recording that is going to be pasted
/// into a specific window, so it belongs on the display that window is on.
/// On a multi-monitor desk the primary display is often not the one being
/// looked at, and a pill on the wrong screen is feedback nobody sees.
///
/// Falls back to the primary monitor, then to whatever monitor exists.
/// `current_monitor()` is not usable here: it returns `None` for a window
/// that is off-screen, which the overlay is until its first reveal.
fn target_monitor(window: &tauri::WebviewWindow) -> Option<tauri::Monitor> {
    #[cfg(windows)]
    if let Some((x, y)) = crate::windows_util::captured_window_center() {
        if let Ok(Some(monitor)) = window.monitor_from_point(x, y) {
            return Some(monitor);
        }
    }
    if let Ok(Some(monitor)) = window.primary_monitor() {
        return Some(monitor);
    }
    // Headless / unusual display setups.
    window
        .available_monitors()
        .ok()
        .and_then(|v| v.into_iter().next())
}

/// На сколько поднять оверлей над нижней кромкой монитора.
const BOTTOM_OFFSET: i32 = 96;

/// Где на мониторе должен стоять оверлей: по центру по горизонтали, у
/// нижнего края с отступом.
///
/// Вынесено из [`position_overlay`] отдельной функцией, потому что это
/// единственная арифметика в модуле, а проверить её на месте было нечем —
/// всё остальное здесь требует настоящего окна и монитора. Ошибка тут
/// ничего не роняет: она тихо ставит оверлей не туда, и заметно это только
/// на втором мониторе или на нестандартном разрешении, то есть у того, кто
/// про это не напишет.
///
/// Всё считается смещением **внутри** монитора и только потом прибавляется
/// к его началу. Клампить абсолютную координату нельзя: у монитора слева от
/// основного начало отрицательное, и `max(0)` над абсолютом выдавил бы
/// оверлей на основной экран.
fn overlay_origin(
    monitor_pos: (i32, i32),
    monitor_size: (u32, u32),
    window_size: (u32, u32),
    bottom_offset: i32,
) -> (i32, i32) {
    let (monitor_x, monitor_y) = monitor_pos;
    let (monitor_w, monitor_h) = monitor_size;
    let (window_w, window_h) = window_size;
    // Окно шире монитора — прижимаем к левой кромке, а не уводим влево.
    let x = monitor_x + ((monitor_w as i32 - window_w as i32) / 2).max(0);
    // Монитор ниже, чем окно с отступом, — прижимаем к верхней кромке.
    let y = monitor_y + (monitor_h as i32 - window_h as i32 - bottom_offset).max(0);
    (x, y)
}

/// Position the overlay window centered horizontally near the bottom of the
/// monitor chosen by [`target_monitor`].
///
/// Returns without touching the window when it already sits at the computed
/// spot. On Windows that is the normal case for every show after the first,
/// and skipping the redundant `SetWindowPos` is what keeps the composition
/// surface undisturbed. A move now also happens when the user dictates into
/// a window on a different display than last time — which is the point.
fn position_overlay(window: &tauri::WebviewWindow) -> Result<(), String> {
    let monitor = target_monitor(window);
    let Some(monitor) = monitor else {
        eprintln!("[overlay] position_overlay: no monitor detected; window stays off-screen");
        return Ok(());
    };
    let monitor_pos = monitor.position();
    let monitor_size = monitor.size();
    let win_size = window.inner_size().map_err(|e| e.to_string())?;
    let (x, y) = overlay_origin(
        (monitor_pos.x, monitor_pos.y),
        (monitor_size.width, monitor_size.height),
        (win_size.width, win_size.height),
        BOTTOM_OFFSET,
    );
    if let Ok(current) = window.outer_position() {
        if current.x == x && current.y == y {
            return Ok(());
        }
    }
    eprintln!(
        "[overlay] position_overlay: monitor=({:?},{:?},{:?}x{:?}) window={:?}x{:?} -> ({x},{y})",
        monitor_pos.x,
        monitor_pos.y,
        monitor_size.width,
        monitor_size.height,
        win_size.width,
        win_size.height
    );
    window
        .set_position(Position::Physical(PhysicalPosition::new(x, y)))
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Переключить оверлей между таблеткой и окном под живой текст.
///
/// Зовётся из окна оверлея, когда диктовка идёт потоковой моделью.
/// Позиция пересчитывается после смены размера: оверлей прижат к низу и
/// выровнен по центру, и без пересчёта он расползался бы вправо и вниз.
#[command]
pub fn set_overlay_streaming(app: AppHandle, streaming: bool) -> Result<(), String> {
    let Some(window) = app.get_webview_window(OVERLAY_LABEL) else {
        return Ok(());
    };
    let (width, height) = if streaming {
        (OVERLAY_STREAM_WIDTH, OVERLAY_STREAM_HEIGHT)
    } else {
        (OVERLAY_WIDTH, OVERLAY_HEIGHT)
    };
    let current = window.inner_size().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().unwrap_or(1.0);
    // Сравнение в физических пикселях: на дробном масштабе логический размер
    // туда-обратно не сходится, и окно дёргалось бы каждый кадр.
    if (current.width as f64 - width * scale).abs() < 1.0
        && (current.height as f64 - height * scale).abs() < 1.0
    {
        return Ok(());
    }
    window
        .set_size(tauri::Size::Logical(tauri::LogicalSize::new(width, height)))
        .map_err(|e| e.to_string())?;
    position_overlay(&window)
}

/// Enqueue a hide. Returns as soon as the op is posted — see the
/// worker-thread comment for why this must never do the work inline. This
/// is the command React calls from its own close button, at the same moment
/// the backend's `whisper-cancelled` listener calls it from a tokio thread;
/// doing the work here is what deadlocked the app.
#[command]
pub fn hide(_app: AppHandle) -> Result<(), String> {
    post(OverlayOp::Hide);
    Ok(())
}

/// Worker-thread body of `OverlayOp::Hide`.
fn apply_hide(app: &AppHandle) -> Result<(), String> {
    eprintln!("[overlay] apply_hide()");
    set_last_state(None);
    // Issue #24: the cancel path is where the flash is reported. If Windows
    // is substituting a stand-in frame for a stalled UI thread, this probe
    // is what says so — and it says it without delaying the hide.
    #[cfg(windows)]
    crate::windows::overlay_diag::probe_main_thread(app, "hide:enter");
    if let Some(window) = app.get_webview_window(OVERLAY_LABEL) {
        // Wipe the previous-recording UI so the next show cannot flash stale
        // DOM (e.g. "23 символов вставлено") before its new state is ready.
        let _ = window.emit("overlay-reset", ());
        match conceal(&window) {
            Ok(()) => {
                set_on_screen(false);
                eprintln!("[overlay] hide: window concealed successfully");
            }
            Err(e) => {
                eprintln!("[overlay] hide: conceal failed: {e}");
                return Err(e);
            }
        }
    } else {
        eprintln!("[overlay] hide: overlay window not found");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Engine-event subscriptions (WS 4a1 Task 15).
//
// Before Task 14, the Python sidecar's `reader_loop` called `sync_overlay`
// synchronously to translate `recording_started` / `transcription_done` /
// `recording_cancelled` etc. into overlay state changes. With the engine
// in charge, those sidecar events no longer fire — the dispatcher in
// `lib.rs::setup` emits `whisper-*` (and `recording-started`) Tauri events
// instead. This module now subscribes to those events and drives the
// overlay via `show_state` / `hide`, matching the Phase 1 mapping
// (recording → processing → done/error) one-to-one.
//
// Event → overlay state mapping (matches the original `sync_overlay`
// table in sidecar.rs):
//
//   `recording-started`       → show_state("recording")   (from `start_recording` Tauri command)
//   `whisper-started`         → show_state("processing")  (InferenceStarted)
//   `whisper-done`            → show_state("done"), no auto-hide (paste pending)
//   `paste-done`              → show_state("pasted") + auto-hide after 1800ms
//   `whisper-failed`          → show_state("error") + auto-hide after 1800ms
//   `whisper-cancelled`       → hide()
//   `whisper-loading`         → show_state("loading")
//   `whisper-load-failed`     → show_state("error") only if currently visible
//
// Auto-hide after 1800ms mirrors the original behavior; it fires only if
// the overlay state is still the same (so a follow-up `recording-started`
// arriving 200ms later isn't preempted by a stale hide).
//
// `whisper-done` is the one state that does NOT auto-hide on that cadence.
// It used to, which is what made a slow LLM look like a finished cycle:
// the overlay announced a character count and vanished while the text was
// still being cleaned up, and the paste landed seconds later into an empty
// screen. Decoding and inserting are now two separate states.
// ---------------------------------------------------------------------------

const AUTO_HIDE_DELAY_MS: u64 = 1800;

/// How long the overlay may sit in "распознано" before we assume the rest
/// of the pipeline died and hide it anyway.
///
/// Not a deadline for the LLM — that has its own timeout, applied per
/// attempt and retried — but a floor under the worst legitimate case, so
/// a genuinely slow model is never cut off mid-flight. What it actually
/// guards against is a paste path that never reports back at all, which
/// would otherwise leave the overlay on screen until the app restarts.
const STUCK_OVERLAY_TIMEOUT_MS: u64 = 180_000;

/// Hide the overlay after `delay_ms`, but only if it is still showing
/// `expected_state`.
///
/// The state check is what makes overlapping timers safe: a newer
/// `recording-started` (or the `paste-done` that follows a `whisper-done`)
/// advances the state, and the older timer then finds a state it does not
/// recognise and does nothing.
fn hide_if_still_showing(expected_state: &'static str, delay_ms: u64) {
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(delay_ms));
        if current_state().as_deref() == Some(expected_state) {
            post(OverlayOp::Hide);
        }
    });
}

/// Subscribe the overlay to engine lifecycle events. Call once from
/// `lib.rs::setup()` after the engine dispatcher task is spawned. The
/// listeners live for the lifetime of the `AppHandle` (i.e. the app
/// process) — there is no unlisten because the listener IDs are tied to
/// the managed state and don't need explicit cleanup.
///
/// The listeners only enqueue: the actual window work happens on the
/// overlay worker thread. They run on tokio workers, so doing it inline is
/// what used to deadlock against React's own `invoke("hide")` on the main
/// thread.
pub fn subscribe_engine_events(app: &AppHandle) {
    // recording-started: emitted by the `start_recording` Tauri command
    // when the UI (not the hotkey) starts a session. Maps to "recording"
    // overlay state.
    app.listen("recording-started", move |_event| {
        post(OverlayOp::Show("recording".to_string()));
    });

    // whisper-started: dispatcher fires this when the engine begins
    // inference. Maps to "processing" — the audio has stopped, the
    // engine is now chewing on it.
    app.listen("whisper-started", move |_event| {
        post(OverlayOp::Show("processing".to_string()));
    });

    // whisper-done: dispatcher fires this on a successful
    // InferenceCompleted — the speech is decoded, but the text has NOT
    // been inserted yet: local formatting and (in hybrid mode) the LLM
    // pass still run, and the latter can take tens of seconds. So this
    // shows "done" without scheduling the usual hide; `paste-done` ends
    // the cycle. The long timer is only a stuck-overlay guard — see
    // `STUCK_OVERLAY_TIMEOUT_MS`.
    app.listen("whisper-done", move |_event| {
        post(OverlayOp::Show("done".to_string()));
        hide_if_still_showing("done", STUCK_OVERLAY_TIMEOUT_MS);
    });

    // paste-done: dispatcher fires this once the text is actually in the
    // focused window. This is the real end of the cycle and the only
    // point where a character count is true.
    app.listen("paste-done", move |_event| {
        post(OverlayOp::Show("pasted".to_string()));
        hide_if_still_showing("pasted", AUTO_HIDE_DELAY_MS);
    });

    // paste-failed: the transcription succeeded but the text never reached
    // the window. Releases the overlay from the "распознано" state it
    // would otherwise hold until the stuck-overlay timeout.
    app.listen("paste-failed", move |_event| {
        post(OverlayOp::Show("error".to_string()));
        hide_if_still_showing("error", AUTO_HIDE_DELAY_MS);
    });

    // whisper-failed: dispatcher fires this on a failed
    // InferenceCompleted. Same auto-hide cadence as done, but with the
    // "error" state.
    app.listen("whisper-failed", move |_event| {
        post(OverlayOp::Show("error".to_string()));
        hide_if_still_showing("error", AUTO_HIDE_DELAY_MS);
    });

    // whisper-empty: dispatcher fires this when a transcription returns
    // empty text — hide the overlay immediately (no "Текст готов" message).
    app.listen("whisper-empty", move |_event| {
        post(OverlayOp::Hide);
    });

    // whisper-cancelled: dispatcher fires this when a cancelled session
    // finishes — hide the overlay immediately. No auto-hide delay
    // because there's nothing for the user to look at.
    app.listen("whisper-cancelled", move |_event| {
        post(OverlayOp::Hide);
    });

    // whisper-loading: dispatcher fires this when the engine starts
    // loading a model. Only worth showing when the user is waiting on it —
    // i.e. a session is already in flight. The auto-load that runs at app
    // startup also fires this, and popping a "Загрузка модели" pill at
    // every launch is just noise (same reasoning as whisper-load-failed).
    app.listen("whisper-loading", move |_event| {
        match current_state().as_deref() {
            Some("recording") | Some("processing") => {
                post(OverlayOp::Show("loading".to_string()));
            }
            _ => eprintln!("[overlay] whisper-loading ignored: no session in flight"),
        }
    });

    // whisper-ready: dispatcher fires this when the engine finishes
    // loading a model successfully (auto-load or user-requested). Hide
    // the overlay since the model is loaded and there's nothing for the
    // user to look at yet. Without this listener, the overlay stays in
    // "loading" state after auto-load completes.
    app.listen("whisper-ready", move |_event| {
        if current_state().as_deref() == Some("loading") {
            post(OverlayOp::Hide);
        }
    });

    // whisper-load-failed: dispatcher fires this when SetModel fails.
    // Only surface the error if the overlay is already showing a
    // relevant state — background model loads (no recording in flight)
    // should not pop the overlay up.
    app.listen("whisper-load-failed", move |_event| {
        match current_state().as_deref() {
            Some("recording") | Some("processing") | Some("loading") => {
                post(OverlayOp::Show("error".to_string()));
            }
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Оверлей 308×64 — размер из tauri.conf.json.
    const OVERLAY: (u32, u32) = (308, 64);
    const FHD: (u32, u32) = (1920, 1080);

    #[test]
    fn the_overlay_is_centred_horizontally() {
        let (x, _y) = overlay_origin((0, 0), FHD, OVERLAY, BOTTOM_OFFSET);
        assert_eq!(x, (1920 - 308) / 2);
    }

    #[test]
    fn the_overlay_sits_above_the_bottom_edge() {
        let (_x, y) = overlay_origin((0, 0), FHD, OVERLAY, BOTTOM_OFFSET);
        assert_eq!(y, 1080 - 64 - BOTTOM_OFFSET);
        assert!(y + 64 < 1080, "оверлей должен не доходить до нижней кромки");
    }

    /// Монитор слева от основного имеет отрицательное начало. Клампить
    /// абсолютную координату здесь означало бы утащить оверлей на основной
    /// экран — то есть не на тот монитор, куда человек диктует.
    #[test]
    fn a_monitor_left_of_the_primary_keeps_the_overlay() {
        let (x, y) = overlay_origin((-1920, 0), FHD, OVERLAY, BOTTOM_OFFSET);
        assert_eq!(x, -1920 + (1920 - 308) / 2);
        assert!(x < 0, "оверлей уехал на основной монитор");
        assert_eq!(y, 1080 - 64 - BOTTOM_OFFSET);
    }

    #[test]
    fn a_monitor_below_the_primary_keeps_the_overlay() {
        let (_x, y) = overlay_origin((0, 1080), FHD, OVERLAY, BOTTOM_OFFSET);
        assert_eq!(y, 1080 + 1080 - 64 - BOTTOM_OFFSET);
    }

    /// Окно шире монитора: прижимаем к левой кромке, а не уводим за неё.
    #[test]
    fn a_window_wider_than_the_monitor_pins_to_the_left_edge() {
        let (x, _y) = overlay_origin((100, 0), (200, 1080), OVERLAY, BOTTOM_OFFSET);
        assert_eq!(x, 100, "ушёл левее монитора");
    }

    /// Монитор ниже, чем окно с отступом: прижимаем к верхней кромке.
    #[test]
    fn a_monitor_shorter_than_the_window_pins_to_the_top_edge() {
        let (_x, y) = overlay_origin((0, 50), (1920, 100), OVERLAY, BOTTOM_OFFSET);
        assert_eq!(y, 50, "ушёл выше монитора");
    }

    /// Масштабирование не должно ломать центровку: координаты физические,
    /// и на 4K оверлей обязан оставаться посередине.
    #[test]
    fn centring_holds_on_a_larger_monitor() {
        let (x, _y) = overlay_origin((0, 0), (3840, 2160), OVERLAY, BOTTOM_OFFSET);
        assert_eq!(x, (3840 - 308) / 2);
    }
}
