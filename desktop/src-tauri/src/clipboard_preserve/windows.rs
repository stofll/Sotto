//! Win32 clipboard: every format whose data is a memory block, copied as bytes.
//!
//! That covers text in every encoding, HTML, RTF, images (`CF_DIB`,
//! `CF_DIBV5`, PNG), copied files (`CF_HDROP` and the shell formats) and
//! application formats. Some formats are skipped on purpose (see
//! [`is_unsupported`]); `CF_BITMAP` is synthesized again from `CF_DIB`,
//! metafile-only content is lost. Any other format that cannot be read fails
//! the capture rather than being dropped.
//!
//! A session is one `OpenClipboard` call. While it is open no other
//! application can change the clipboard, so the ownership check and the
//! write it guards cannot be separated by someone else's copy.
//!
//! Ownership is an [`OWNER_FORMAT`] mark, not the clipboard sequence number.
//! The number cannot be read inside a session: closing the clipboard adds the
//! synthesized formats (`CF_TEXT`, `CF_OEMTEXT`, `CF_LOCALE`) and bumps it
//! again, so the value seen while it is open never matches the one later.
//! The mark needs no such guess: every write by another application starts
//! with `EmptyClipboard`, which removes it.

use std::sync::atomic::{AtomicU64, Ordering};

use tauri::AppHandle;
use windows_sys::Win32::Foundation::{GetLastError, GlobalFree, SetLastError, HANDLE, HGLOBAL};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardFormatNameW, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};

const CF_UNICODETEXT: u32 = 13;

/// A private format on every clipboard state Sotto writes, holding a value
/// unique to that write.
const OWNER_FORMAT: &str = "Sotto.ClipboardOwner";

/// Formats deliberately left out of a capture.
///
/// - GDI handles (bitmaps, palettes, metafiles) are not memory blocks;
///   `CF_BITMAP` comes back from `CF_DIB`.
/// - The private range `CF_PRIVATEFIRST..=CF_PRIVATELAST` holds whatever
///   handle its owner chose, and the system does not manage it.
/// - OLE's own bookkeeping is an interface pointer into the process that
///   copied; put back after that owner is gone it would point at nothing, and
///   OLE rebuilds its data object from the other formats.
/// - Sotto's own [`OWNER_FORMAT`] mark is written afresh, never copied.
fn is_unsupported(format: u32) -> bool {
    const CF_BITMAP: u32 = 2;
    const CF_METAFILEPICT: u32 = 3;
    const CF_PALETTE: u32 = 9;
    const CF_ENHMETAFILE: u32 = 14;
    const CF_OWNERDISPLAY: u32 = 0x80;
    const CF_DSPBITMAP: u32 = 0x82;
    const CF_DSPMETAFILEPICT: u32 = 0x83;
    const CF_DSPENHMETAFILE: u32 = 0x8E;
    const CF_PRIVATEFIRST: u32 = 0x200;
    const CF_GDIOBJLAST: u32 = 0x3FF;
    const OLE_FORMATS: [&str; 2] = ["DataObject", "Ole Private Data"];
    matches!(
        format,
        CF_BITMAP
            | CF_METAFILEPICT
            | CF_PALETTE
            | CF_ENHMETAFILE
            | CF_OWNERDISPLAY
            | CF_DSPBITMAP
            | CF_DSPMETAFILEPICT
            | CF_DSPENHMETAFILE
    ) || (CF_PRIVATEFIRST..=CF_GDIOBJLAST).contains(&format)
        || (format >= 0xC000
            && format_name(format)
                .is_some_and(|name| name == OWNER_FORMAT || OLE_FORMATS.contains(&name.as_str())))
}

/// Keep the dictated text, and the restored copy of contents they already
/// recorded, out of Win+V history, cloud clipboard and clipboard managers.
const MONITOR_EXCLUSIONS: [(&str, u32); 3] = [
    ("ExcludeClipboardContentFromMonitorProcessing", 0),
    ("CanIncludeInClipboardHistory", 0),
    ("CanUploadToCloudClipboard", 0),
];

pub struct Clipboard;

impl Clipboard {
    pub fn new(_app: &AppHandle) -> Self {
        Self
    }
}

impl super::Store for Clipboard {
    type Session = Held;

    fn open(&self) -> Result<Held, String> {
        // Another process (often the paste target reading our text) may hold
        // it open for a moment.
        for _ in 0..20 {
            // SAFETY: plain Win32 call; a null owner is accepted.
            if unsafe { OpenClipboard(0) } != 0 {
                return Ok(Held);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Err("clipboard is held by another application".into())
    }
}

/// The clipboard, open for this thread until dropped.
pub struct Held;

impl Drop for Held {
    fn drop(&mut self) {
        // SAFETY: balances the successful `OpenClipboard` in `Clipboard::open`.
        unsafe { CloseClipboard() };
    }
}

/// The [`OWNER_FORMAT`] value on the clipboard. Equal only to the same
/// write: a clipboard without the mark belongs to someone else, including
/// to another clipboard without it.
#[derive(Debug, Clone, Copy)]
pub struct Owner(Option<u64>);

impl PartialEq for Owner {
    fn eq(&self, other: &Self) -> bool {
        matches!((self.0, other.0), (Some(ours), Some(theirs)) if ours == theirs)
    }
}

fn next_mark() -> u64 {
    // Seeded from the clock so a restarted Sotto does not reuse old marks.
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos() as u64)
        .unwrap_or(1);
    let _ = NEXT.compare_exchange(0, seed, Ordering::Relaxed, Ordering::Relaxed);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn register(name: &str) -> u32 {
    let name = wide(name);
    // SAFETY: `name` is a NUL-terminated UTF-16 string that outlives the call.
    unsafe { RegisterClipboardFormatW(name.as_ptr()) }
}

fn format_name(format: u32) -> Option<String> {
    let mut buffer = [0u16; 64];
    // SAFETY: the buffer length passed is its real length.
    let len = unsafe { GetClipboardFormatNameW(format, buffer.as_mut_ptr(), buffer.len() as i32) };
    (len > 0).then(|| String::from_utf16_lossy(&buffer[..len as usize]))
}

/// The bytes of `format`. Only call while the clipboard is open.
fn read_block(format: u32) -> Result<Vec<u8>, String> {
    // SAFETY: the clipboard is open; the block is locked for the copy only
    // and the copy is bounded by `GlobalSize`.
    unsafe {
        let handle = GetClipboardData(format);
        if handle == 0 {
            return Err(format!("format {format} could not be read"));
        }
        let block = handle as HGLOBAL;
        let size = GlobalSize(block);
        let data = GlobalLock(block) as *const u8;
        if size == 0 || data.is_null() {
            if !data.is_null() {
                GlobalUnlock(block);
            }
            return Err(format!("format {format} is not a readable memory block"));
        }
        let bytes = std::slice::from_raw_parts(data, size).to_vec();
        GlobalUnlock(block);
        Ok(bytes)
    }
}

/// Put `bytes` on the clipboard as `format`. Only call while it is open and
/// owned by us, i.e. after `EmptyClipboard`.
fn write_block(format: u32, bytes: &[u8]) -> Result<(), String> {
    // SAFETY: the block is allocated with room for `bytes`, written while
    // locked, and freed only if the clipboard did not take it over.
    unsafe {
        let block = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1));
        if block.is_null() {
            return Err("out of memory".into());
        }
        let data = GlobalLock(block) as *mut u8;
        if data.is_null() {
            GlobalFree(block);
            return Err("could not lock clipboard memory".into());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
        GlobalUnlock(block);
        if SetClipboardData(format, block as HANDLE) == 0 {
            GlobalFree(block);
            return Err(format!("could not set clipboard format {format}"));
        }
        Ok(())
    }
}

/// Best effort: a clipboard history recording the text is a privacy leak,
/// not a reason to refuse the paste.
fn write_exclusions(present: &[u32]) {
    for (name, value) in MONITOR_EXCLUSIONS {
        let format = register(name);
        if present.contains(&format) {
            continue;
        }
        if let Err(error) = write_block(format, &value.to_le_bytes()) {
            log::debug!("clipboard marker {name}: {error}");
        }
    }
}

/// Empty the clipboard and mark it as Sotto's. Written first, so that a
/// write failing part-way still leaves a state Sotto may restore over.
fn empty_and_mark() -> Result<(), String> {
    // SAFETY: called with the clipboard open.
    if unsafe { EmptyClipboard() } == 0 {
        return Err("could not empty the clipboard".into());
    }
    write_block(register(OWNER_FORMAT), &next_mark().to_le_bytes())
}

impl super::Session for Held {
    /// Formats in the order the owner offered them: richest first.
    type Snapshot = Vec<(u32, Vec<u8>)>;
    type Stamp = Owner;

    fn stamp(&self) -> Owner {
        let mark = read_block(register(OWNER_FORMAT))
            .ok()
            .and_then(|bytes| Some(u64::from_le_bytes(bytes.get(..8)?.try_into().ok()?)));
        Owner(mark)
    }

    fn capture(&self) -> Result<Self::Snapshot, String> {
        let mut formats = Vec::new();
        let mut format = 0;
        loop {
            // SAFETY: the clipboard is open. `EnumClipboardFormats` reports
            // both the end of the list and an error as 0; only the error
            // sets the last error.
            format = unsafe {
                SetLastError(0);
                EnumClipboardFormats(format)
            };
            if format == 0 {
                // SAFETY: plain Win32 call.
                let error = unsafe { GetLastError() };
                if error != 0 {
                    return Err(format!("could not list clipboard formats (error {error})"));
                }
                break;
            }
            if !is_unsupported(format) {
                formats.push((format, read_block(format)?));
            }
        }
        Ok(formats)
    }

    fn replace_with_text(&mut self, text: &str) -> Result<(), String> {
        empty_and_mark()?;
        let bytes: Vec<u8> = wide(text)
            .iter()
            .flat_map(|unit| unit.to_le_bytes())
            .collect();
        write_block(CF_UNICODETEXT, &bytes)?;
        write_exclusions(&[]);
        Ok(())
    }

    fn replace_with(&mut self, snapshot: &Self::Snapshot) -> Result<(), String> {
        // An empty clipboard stays empty: no mark or markers on it.
        if snapshot.is_empty() {
            // SAFETY: called with the clipboard open.
            if unsafe { EmptyClipboard() } == 0 {
                return Err("could not empty the clipboard".into());
            }
            return Ok(());
        }
        let marked = empty_and_mark();
        let failed: Vec<u32> = snapshot
            .iter()
            .filter(|(format, bytes)| write_block(*format, bytes).is_err())
            .map(|(format, _)| *format)
            .collect();
        let present: Vec<u32> = snapshot.iter().map(|(format, _)| *format).collect();
        write_exclusions(&present);
        marked?;
        if failed.is_empty() {
            Ok(())
        } else {
            Err(format!("clipboard formats not restored: {failed:?}"))
        }
    }
}

/// These use the real system clipboard, which is shared with the desktop and
/// with every other test, so they are opt-in:
/// `cargo test clipboard_preserve -- --ignored --test-threads=1`.
/// They save the clipboard first and put it back at the end.
#[cfg(test)]
mod tests {
    use super::super::{Session, Store};
    use super::*;

    const CF_DIB: u32 = 8;
    const CF_HDROP: u32 = 15;

    /// What another application's copy does: empty, then write.
    fn foreign_copy(formats: &[(u32, Vec<u8>)]) {
        let _held = Clipboard.open().unwrap();
        // SAFETY: the clipboard is open.
        assert_ne!(unsafe { EmptyClipboard() }, 0);
        for (format, bytes) in formats {
            write_block(*format, bytes).unwrap();
        }
    }

    fn capture() -> Vec<(u32, Vec<u8>)> {
        Clipboard.open().unwrap().capture().unwrap()
    }

    fn utf16(text: &str) -> Vec<u8> {
        wide(text)
            .iter()
            .flat_map(|unit| unit.to_le_bytes())
            .collect()
    }

    /// A `DROPFILES` header followed by a double-NUL-terminated wide path list.
    fn dropped_file(path: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&20u32.to_le_bytes()); // pFiles
        bytes.extend_from_slice(&[0; 12]); // pt, fNC
        bytes.extend_from_slice(&1u32.to_le_bytes()); // fWide
        bytes.extend(utf16(path));
        bytes.extend_from_slice(&[0, 0]);
        bytes
    }

    /// A 1×1 32-bit `BITMAPINFOHEADER` image.
    fn one_pixel_dib() -> Vec<u8> {
        let mut bytes = Vec::new();
        for field in [40u32, 1, 1] {
            bytes.extend_from_slice(&field.to_le_bytes());
        }
        bytes.extend_from_slice(&1u16.to_le_bytes()); // planes
        bytes.extend_from_slice(&32u16.to_le_bytes()); // bit count
        bytes.extend_from_slice(&[0; 24]); // compression .. clr important
        bytes.extend_from_slice(&[0x10, 0x20, 0x30, 0xFF]);
        bytes
    }

    /// GlobalSize may round a block up, so a restored block may be longer.
    fn holds(snapshot: &[(u32, Vec<u8>)], format: u32, bytes: &[u8]) -> bool {
        snapshot
            .iter()
            .any(|(saved, data)| *saved == format && data.starts_with(bytes))
    }

    /// Paste `text` the way a preserving paste does: capture, then replace,
    /// in one session.
    fn paste(text: &str) -> (Vec<(u32, Vec<u8>)>, Owner) {
        let mut held = Clipboard.open().unwrap();
        let snapshot = held.capture().unwrap();
        held.replace_with_text(text).unwrap();
        (snapshot, held.stamp())
    }

    #[test]
    fn gdi_private_and_ole_formats_are_skipped_on_purpose() {
        for format in [
            2, 3, 9, 14, 0x80, 0x82, 0x83, 0x8E, 0x200, 0x2FF, 0x300, 0x3FF,
        ] {
            assert!(is_unsupported(format), "{format:#x}");
        }
        assert!(is_unsupported(register(OWNER_FORMAT)));
    }

    #[test]
    fn text_image_and_file_formats_are_captured() {
        // CF_TEXT, CF_DIB, CF_UNICODETEXT, CF_HDROP, CF_LOCALE, CF_DIBV5.
        for format in [1, 8, 13, 15, 16, 17] {
            assert!(!is_unsupported(format), "{format:#x}");
        }
        assert!(!is_unsupported(register("HTML Format")));
    }

    #[test]
    fn a_missing_mark_never_counts_as_ours() {
        assert_ne!(Owner(None), Owner(None));
        assert_ne!(Owner(Some(1)), Owner(None));
        assert_ne!(Owner(Some(1)), Owner(Some(2)));
        assert_eq!(Owner(Some(1)), Owner(Some(1)));
    }

    #[test]
    #[ignore = "uses the live system clipboard"]
    fn text_html_files_and_images_survive_a_paste() {
        let users = capture();

        let html = register("HTML Format");
        let text = utf16("Кириллица, Latin, №1 — «спец» <&> \"' \t🙂");
        let page = b"Version:0.9\r\n<b>bold</b>".to_vec();
        let file = dropped_file(r"C:\Temp\пример.txt");
        let image = one_pixel_dib();
        foreign_copy(&[
            (CF_UNICODETEXT, text.clone()),
            (html, page.clone()),
            (CF_HDROP, file.clone()),
            (CF_DIB, image.clone()),
        ]);

        let (snapshot, stamp) = paste("диктовка");
        let pasted = crate::windows_util::clipboard_contains("диктовка");
        // Closing the clipboard adds synthesized formats; ownership survives.
        let still_ours = Clipboard.open().unwrap().stamp() == stamp;
        Clipboard.open().unwrap().replace_with(&snapshot).unwrap();
        let after = capture();

        Clipboard.open().unwrap().replace_with(&users).unwrap();
        assert!(pasted);
        assert!(still_ours, "Sotto's own write must still count as its own");
        assert!(holds(&after, CF_UNICODETEXT, &text));
        assert!(holds(&after, html, &page));
        assert!(holds(&after, CF_HDROP, &file));
        assert!(holds(&after, CF_DIB, &image));
    }

    #[test]
    #[ignore = "uses the live system clipboard"]
    fn an_empty_clipboard_stays_empty_and_a_newer_copy_is_detected() {
        let users = capture();

        foreign_copy(&[]);
        let (snapshot, stamp) = paste("диктовка");
        Clipboard.open().unwrap().replace_with(&snapshot).unwrap();
        let after = capture();
        let (_, stamp_again) = paste("диктовка");
        foreign_copy(&[(CF_UNICODETEXT, utf16("диктовка"))]);
        // Even a copy of the very same text is someone else's.
        let changed = Clipboard.open().unwrap().stamp() != stamp_again;

        Clipboard.open().unwrap().replace_with(&users).unwrap();
        assert!(snapshot.is_empty());
        assert!(after.is_empty(), "{after:?}");
        assert!(stamp != stamp_again, "each write carries its own mark");
        assert!(
            changed,
            "another application's copy must not look like ours"
        );
    }
}
