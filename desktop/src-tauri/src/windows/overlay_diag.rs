//! Instrumentation for issue #24 — the native `Overlay` title bar that
//! flashes on the first cancel after launch.
//!
//! Ten style- and DWM-level mitigations have been tried and none removed
//! the flash (see the experiment log in the issue). That pattern says the
//! next useful step is not an eleventh guess but data, and specifically
//! data that separates two hypotheses no experiment has yet distinguished:
//!
//! 1. **tao restores the caption.** The overlay is built `visible(false)`
//!    and its styles are then rewritten behind tao's back. Any tao-driven
//!    visibility change runs `WindowFlags::apply_diff`, which recomputes
//!    the style set from tao's own cached flags — which still contain
//!    `WS_OVERLAPPEDWINDOW`. A deferred first `set_visible` would do this
//!    exactly once per process, which matches "first cycle only".
//!
//! 2. **The window is not ours.** When a top-level window stops pumping
//!    messages for ~5 s, USER32 substitutes a decorated stand-in carrying
//!    the same title. That would explain the caption reading `Overlay`,
//!    the real frame, the immunity to every style change we make to our
//!    own HWND, and how quickly it disappears.
//!
//! The two call for opposite fixes — a window-proc subclass versus getting
//! work off the main thread — so guessing costs another round trip through
//! a packaged build. `snapshot` answers (1): it records the style bits at
//! every transition, so a caption that comes back between two of our own
//! calls is visible as a diff. `enumerate_top_level` answers (2): a frame
//! painted by a window whose class is not tao's is not our window at all.
//!
//! Off unless `debug_overlay_diag` is set in config, because it is verbose
//! and because it enumerates window titles, which are user data.
#![cfg(windows)]

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::Graphics::Dwm::DwmGetWindowAttribute;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowLongPtrW, GetWindowRect, GetWindowTextW,
    GetWindowThreadProcessId, IsHungAppWindow, IsWindowVisible, GWL_EXSTYLE, GWL_STYLE,
};

const DWMWA_CLOAKED_ATTR: u32 = 14;

/// Config key. Off by default: the enumeration below reads the titles of
/// every top-level window on the desktop, which is other people's data.
const CONFIG_KEY: &str = "debug_overlay_diag";

static ENABLED: AtomicBool = AtomicBool::new(false);
/// Which show/hide cycle we are in. The defect is first-cycle-only, so the
/// cycle number is the first thing to look for in a log.
static CYCLE: AtomicU32 = AtomicU32::new(0);

pub fn configure(config: &serde_json::Value) {
    let on = config
        .get(CONFIG_KEY)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    ENABLED.store(on, Ordering::Release);
    if on {
        log::info!("overlay-diag: enabled (issue #24 instrumentation)");
    }
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

/// Called once per hidden→shown transition, so every line can be attributed
/// to a cycle. Everything interesting happens in cycle 1.
pub fn begin_cycle() -> u32 {
    CYCLE.fetch_add(1, Ordering::AcqRel) + 1
}

pub fn cycle() -> u32 {
    CYCLE.load(Ordering::Acquire)
}

/// Decode a style word into the names of the bits that are set.
///
/// Printed as names rather than a hex word on purpose: the whole question
/// is "did `WS_CAPTION` come back between these two lines", and nobody
/// reads that reliably out of `0x16CF0000`. The raw word is kept alongside
/// them — that is what gets pasted into the issue.
fn describe_bits(value: isize, bits: &[(isize, &str)]) -> String {
    let mut found: Vec<&str> = bits
        .iter()
        // Whole mask, not any-bit: WS_CAPTION is BORDER|DLGFRAME, and a
        // plain `& != 0` test would report it for a lone WS_BORDER.
        .filter(|(bit, _)| (value & *bit) == *bit)
        .map(|(_, name)| *name)
        .collect();
    if found.is_empty() {
        found.push("none");
    }
    format!("0x{:08X} [{}]", value as u32, found.join("|"))
}

/// The style bits that decide whether Windows paints a frame.
///
/// `WS_OVERLAPPED` is deliberately absent: it is `0`, so every window
/// matches it and it would say nothing. An empty list reads as `none`.
fn describe_style(style: isize) -> String {
    describe_bits(
        style,
        &[
            (0x0080_0000, "WS_BORDER"),
            (0x00C0_0000, "WS_CAPTION"),
            (0x0040_0000, "WS_DLGFRAME"),
            (0x0004_0000, "WS_THICKFRAME"),
            (0x0008_0000, "WS_SYSMENU"),
            (0x0001_0000, "WS_MAXIMIZEBOX"),
            (0x0002_0000, "WS_MINIMIZEBOX"),
            (0x1000_0000, "WS_VISIBLE"),
            (0x8000_0000_u32 as isize, "WS_POPUP"),
        ],
    )
}

fn describe_exstyle(exstyle: isize) -> String {
    describe_bits(
        exstyle,
        &[
            (0x0000_0080, "WS_EX_TOOLWINDOW"),
            (0x0800_0000, "WS_EX_NOACTIVATE"),
            (0x0000_0020, "WS_EX_TRANSPARENT"),
            (0x0000_0008, "WS_EX_TOPMOST"),
            (0x0002_0000, "WS_EX_STATICEDGE"),
            (0x0000_0300, "WS_EX_WINDOWEDGE|CLIENTEDGE"),
            (0x0000_0001, "WS_EX_DLGMODALFRAME"),
            (0x0020_0000, "WS_EX_LAYERED"),
        ],
    )
}

fn wide_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if len <= 0 {
        return "<unknown>".to_string();
    }
    wide_to_string(&buf[..len as usize])
}

fn window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let len = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if len <= 0 {
        return String::new();
    }
    wide_to_string(&buf[..len as usize])
}

/// DWM's own view of whether the window is presented. Distinguishes "we
/// asked for a cloak and it took" from "the cloak call returned success and
/// DWM ignored it", which the existing code cannot tell apart.
fn cloaked(hwnd: HWND) -> String {
    let mut value: u32 = 0;
    let hr = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED_ATTR,
            &mut value as *mut _ as *mut std::ffi::c_void,
            std::mem::size_of_val(&value) as u32,
        )
    };
    if hr < 0 {
        return format!("query-failed(0x{hr:08X})");
    }
    match value {
        0 => "no".to_string(),
        1 => "yes(app)".to_string(),
        2 => "yes(shell)".to_string(),
        4 => "yes(inherited)".to_string(),
        other => format!("yes({other})"),
    }
}

/// One line per transition: everything about our own HWND that could
/// explain a frame appearing.
///
/// `label` names the exact point in `overlay.rs`. Two adjacent snapshots
/// that differ in `WS_CAPTION` name the call that put it back — which is
/// the whole point of the instrumentation.
pub fn snapshot(hwnd: HWND, label: &str) {
    if !enabled() {
        return;
    }
    let (style, exstyle, visible, hung, thread) = unsafe {
        (
            GetWindowLongPtrW(hwnd, GWL_STYLE),
            GetWindowLongPtrW(hwnd, GWL_EXSTYLE),
            IsWindowVisible(hwnd) != 0,
            IsHungAppWindow(hwnd) != 0,
            GetWindowThreadProcessId(hwnd, std::ptr::null_mut()),
        )
    };
    let mut rect = unsafe { std::mem::zeroed() };
    let have_rect = unsafe { GetWindowRect(hwnd, &mut rect) != 0 };
    let rect_text = if have_rect {
        format!(
            "{}x{}@{},{}",
            rect.right - rect.left,
            rect.bottom - rect.top,
            rect.left,
            rect.top
        )
    } else {
        "<no rect>".to_string()
    };

    log::info!(
        "overlay-diag cycle={} at={label} hwnd=0x{:X} class={} title={:?} \
         style={} exstyle={} visible={visible} cloaked={} hung={hung} \
         rect={rect_text} owner_thread={thread} this_thread={:?}",
        cycle(),
        hwnd as usize,
        class_name(hwnd),
        window_title(hwnd),
        describe_style(style),
        describe_exstyle(exstyle),
        cloaked(hwnd),
        std::thread::current().id(),
    );
}

struct EnumState {
    ours: HWND,
    found: Vec<String>,
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> i32 {
    let state = &mut *(lparam as *mut EnumState);
    if IsWindowVisible(hwnd) == 0 {
        return 1;
    }
    let mut pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    let title = window_title(hwnd);
    let class = class_name(hwnd);

    // Two things are worth reporting, and nothing else: windows belonging
    // to this process, and any window anywhere whose title mentions the
    // overlay. The second clause is the one that catches an impostor — a
    // stand-in frame carries our title but need not be in our process.
    let ours = pid == std::process::id() || title.contains("Overlay");
    if !ours {
        return 1;
    }
    let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
    let exstyle = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    state.found.push(format!(
        "hwnd=0x{:X}{} class={class} title={title:?} pid={pid} style={} exstyle={} hung={}",
        hwnd as usize,
        if hwnd == state.ours { " <-- OURS" } else { "" },
        describe_style(style),
        describe_exstyle(exstyle),
        IsHungAppWindow(hwnd) != 0,
    ));
    1
}

/// Every visible top-level window of this process, plus any window titled
/// `Overlay` regardless of owner.
///
/// This is the line that settles the question. If the caption belongs to a
/// window whose class is not the overlay's, no amount of restyling our HWND
/// will ever remove it, and the ten failed experiments are explained.
pub fn enumerate_top_level(ours: HWND, label: &str) {
    if !enabled() {
        return;
    }
    let mut state = EnumState {
        ours,
        found: Vec::new(),
    };
    unsafe {
        EnumWindows(Some(enum_proc), &mut state as *mut _ as LPARAM);
    }
    log::info!(
        "overlay-diag cycle={} at={label} top-level windows ({}):",
        cycle(),
        state.found.len()
    );
    for line in &state.found {
        log::info!("overlay-diag   {line}");
    }
}

/// Report how long the main thread takes to run a trivial closure.
///
/// Windows substitutes a decorated stand-in for a top-level window whose
/// thread has not pumped messages for about five seconds. If that is what
/// the flash is, this number goes past 5000 ms during the first cancel and
/// the fix is to move work off the main thread — not to touch styles again.
/// If it stays in the tens of milliseconds, the substitution theory is dead
/// and the style log above is where the answer is.
///
/// Returns immediately and reports from its own thread. A probe that waited
/// for its own answer would hold up the very show/hide it is measuring, and
/// a measurement that changes the timing of the defect is worse than none.
pub fn probe_main_thread(app: &tauri::AppHandle, label: &'static str) {
    if !enabled() {
        return;
    }
    let app = app.clone();
    let at_cycle = cycle();
    std::thread::spawn(move || {
        let started = Instant::now();
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        if let Err(e) = app.run_on_main_thread(move || {
            let _ = tx.send(());
        }) {
            log::warn!("overlay-diag: main-thread probe could not be dispatched: {e}");
            return;
        }
        // Bounded so the probe can never outlive the question. The timeout
        // sits above the ~5 s substitution threshold on purpose: a probe
        // that gave up at 1 s could not tell a slow main thread from a
        // stuck one, and that is the only distinction it exists to make.
        match rx.recv_timeout(Duration::from_millis(8_000)) {
            Ok(()) => log::info!(
                "overlay-diag cycle={at_cycle} at={label} main-thread round-trip {} ms",
                started.elapsed().as_millis()
            ),
            Err(_) => log::warn!(
                "overlay-diag cycle={at_cycle} at={label} MAIN THREAD DID NOT RESPOND within \
                 8000 ms — past the window-substitution threshold"
            ),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caption_is_reported_only_when_both_of_its_bits_are_set() {
        // WS_CAPTION is WS_BORDER | WS_DLGFRAME. A naive `style & bit != 0`
        // test reports "caption" for a window that merely has a border,
        // which would turn every diagnostic line into a false positive on
        // exactly the bit this instrumentation exists to watch.
        let border_only = 0x0080_0000isize;
        assert!(
            !describe_style(border_only).contains("WS_CAPTION"),
            "WS_BORDER alone must not be reported as a caption"
        );

        let caption = 0x00C0_0000isize;
        assert!(
            describe_style(caption).contains("WS_CAPTION"),
            "BORDER|DLGFRAME must be reported as a caption"
        );
    }

    #[test]
    fn style_description_keeps_the_raw_word() {
        // The decoded names are for reading; the hex is what gets pasted
        // into the issue and compared against Microsoft's tables.
        let text = describe_style(0x00C0_0000isize);
        assert!(
            text.contains("0x00C00000"),
            "the raw style word must survive decoding, got: {text}"
        );
    }

    #[test]
    fn a_stripped_popup_reports_no_caption() {
        // The state `apply_noactivate_styles` is supposed to leave behind.
        // If this ever reports a caption, the diagnostic would accuse the
        // code that just did its job correctly.
        let popup = 0x8000_0000_u32 as isize;
        let text = describe_style(popup);
        assert!(text.contains("WS_POPUP"), "got: {text}");
        assert!(!text.contains("WS_CAPTION"), "got: {text}");
    }

    #[test]
    fn exstyle_names_the_overlay_flags() {
        let text = describe_exstyle(0x0800_0000 | 0x0000_0080);
        assert!(text.contains("WS_EX_NOACTIVATE"), "got: {text}");
        assert!(text.contains("WS_EX_TOOLWINDOW"), "got: {text}");
    }

    #[test]
    fn diagnostics_are_off_until_configured() {
        // The enumeration reads titles of windows belonging to other
        // applications. That must never happen because someone forgot a
        // default.
        configure(&serde_json::json!({}));
        assert!(!enabled(), "absent config key must leave diagnostics off");

        configure(&serde_json::json!({ "debug_overlay_diag": false }));
        assert!(!enabled(), "explicit false must leave diagnostics off");

        configure(&serde_json::json!({ "debug_overlay_diag": true }));
        assert!(enabled(), "explicit true must turn diagnostics on");

        // Leave the global as the rest of the suite expects to find it.
        configure(&serde_json::json!({}));
    }
}
