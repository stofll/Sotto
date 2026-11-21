//! Win32 clipboard: every format whose data is a memory block, copied as bytes.
//!
//! That covers text in every encoding, HTML, RTF, images (`CF_DIB`,
//! `CF_DIBV5`, PNG), copied files (`CF_HDROP` and the shell formats) and
//! application formats. GDI handle formats cannot be copied as bytes;
//! `CF_BITMAP` is synthesized again from `CF_DIB`, metafiles are lost.

use tauri::AppHandle;
use windows_sys::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardFormatNameW, GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW,
    SetClipboardData,
};
use windows_sys::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};

const CF_UNICODETEXT: u32 = 13;

/// Formats holding a GDI handle rather than a memory block.
fn is_handle_format(format: u32) -> bool {
    const CF_BITMAP: u32 = 2;
    const CF_METAFILEPICT: u32 = 3;
    const CF_PALETTE: u32 = 9;
    const CF_ENHMETAFILE: u32 = 14;
    const CF_OWNERDISPLAY: u32 = 0x80;
    const CF_DSPBITMAP: u32 = 0x82;
    const CF_DSPMETAFILEPICT: u32 = 0x83;
    const CF_DSPENHMETAFILE: u32 = 0x8E;
    const CF_GDIOBJFIRST: u32 = 0x300;
    const CF_GDIOBJLAST: u32 = 0x3FF;
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
    ) || (CF_GDIOBJFIRST..=CF_GDIOBJLAST).contains(&format)
}

/// OLE's own bookkeeping: an interface pointer into the process that copied.
/// Put back after that owner is gone, it would point at nothing; OLE builds
/// its data object from the other formats instead.
const OLE_FORMATS: [&str; 2] = ["DataObject", "Ole Private Data"];

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

/// The clipboard, open for this thread until dropped.
struct Open;

impl Open {
    fn new() -> Result<Self, String> {
        // Another process (often the paste target reading our text) may hold
        // it open for a moment.
        for _ in 0..20 {
            // SAFETY: plain Win32 call; a null owner is accepted.
            if unsafe { OpenClipboard(0) } != 0 {
                return Ok(Self);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Err("clipboard is held by another application".into())
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        // SAFETY: balances the successful `OpenClipboard` in `Open::new`.
        unsafe { CloseClipboard() };
    }
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
fn read_block(format: u32) -> Option<Vec<u8>> {
    // SAFETY: the clipboard is open; the block is locked for the copy only
    // and the copy is bounded by `GlobalSize`.
    unsafe {
        let handle = GetClipboardData(format);
        if handle == 0 {
            return None;
        }
        let block = handle as HGLOBAL;
        let size = GlobalSize(block);
        if size == 0 {
            return None;
        }
        let data = GlobalLock(block) as *const u8;
        if data.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(data, size).to_vec();
        GlobalUnlock(block);
        Some(bytes)
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

fn write_exclusions() {
    for (name, value) in MONITOR_EXCLUSIONS {
        if let Err(error) = write_block(register(name), &value.to_le_bytes()) {
            log::debug!("clipboard marker {name}: {error}");
        }
    }
}

fn empty() -> Result<(), String> {
    // SAFETY: called with the clipboard open.
    if unsafe { EmptyClipboard() } == 0 {
        return Err("could not empty the clipboard".into());
    }
    Ok(())
}

impl super::Store for Clipboard {
    /// Formats in the order the owner offered them: richest first.
    type Snapshot = Vec<(u32, Vec<u8>)>;
    /// `GetClipboardSequenceNumber` right after our write.
    type Stamp = u32;

    fn capture(&self) -> Result<Self::Snapshot, String> {
        let _open = Open::new()?;
        let mut formats = Vec::new();
        let mut skipped = Vec::new();
        let mut format = 0;
        loop {
            // SAFETY: the clipboard is open.
            format = unsafe { EnumClipboardFormats(format) };
            if format == 0 {
                break;
            }
            if is_handle_format(format) {
                continue;
            }
            if format >= 0xC000
                && format_name(format).is_some_and(|name| OLE_FORMATS.contains(&name.as_str()))
            {
                continue;
            }
            match read_block(format) {
                Some(bytes) => formats.push((format, bytes)),
                None => skipped.push(format),
            }
        }
        if !skipped.is_empty() {
            log::warn!("clipboard formats not saved: {skipped:?}");
        }
        Ok(formats)
    }

    fn write_text(&self, text: &str) -> Result<u32, String> {
        {
            let _open = Open::new()?;
            empty()?;
            let utf16 = wide(text);
            // SAFETY: reinterpreting initialized u16s as their bytes.
            let bytes =
                unsafe { std::slice::from_raw_parts(utf16.as_ptr() as *const u8, utf16.len() * 2) };
            write_block(CF_UNICODETEXT, bytes)?;
            write_exclusions();
        }
        // SAFETY: plain Win32 call.
        Ok(unsafe { GetClipboardSequenceNumber() })
    }

    fn still_ours(&self, stamp: &u32) -> bool {
        // SAFETY: plain Win32 call.
        unsafe { GetClipboardSequenceNumber() == *stamp }
    }

    fn restore(&self, snapshot: &Self::Snapshot) -> Result<(), String> {
        let _open = Open::new()?;
        empty()?;
        let mut failed = Vec::new();
        for (format, bytes) in snapshot {
            if write_block(*format, bytes).is_err() {
                failed.push(*format);
            }
        }
        if !failed.is_empty() {
            log::warn!("clipboard formats not restored: {failed:?}");
        }
        // An empty clipboard stays empty: no markers on it.
        if !snapshot.is_empty() {
            let present: Vec<u32> = snapshot.iter().map(|(format, _)| *format).collect();
            for (name, value) in MONITOR_EXCLUSIONS {
                let format = register(name);
                if !present.contains(&format) {
                    let _ = write_block(format, &value.to_le_bytes());
                }
            }
        }
        Ok(())
    }
}

/// These use the real system clipboard, which is shared with the desktop and
/// with every other test, so they are opt-in:
/// `cargo test clipboard_preserve -- --ignored --test-threads=1`.
/// They save the clipboard first and put it back at the end.
#[cfg(test)]
mod tests {
    use super::super::Store;
    use super::*;

    const CF_DIB: u32 = 8;
    const CF_HDROP: u32 = 15;

    fn set(formats: &[(u32, Vec<u8>)]) {
        let _open = Open::new().unwrap();
        empty().unwrap();
        for (format, bytes) in formats {
            write_block(*format, bytes).unwrap();
        }
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

    #[test]
    #[ignore = "uses the live system clipboard"]
    fn text_html_files_and_images_survive_a_paste() {
        let clipboard = Clipboard;
        let users = clipboard.capture().unwrap();

        let html = register("HTML Format");
        let text = utf16("Кириллица, Latin, №1 — «спец» <&> \"' \t🙂");
        let page = b"Version:0.9\r\n<b>bold</b>".to_vec();
        let file = dropped_file(r"C:\Temp\пример.txt");
        let image = one_pixel_dib();
        set(&[
            (CF_UNICODETEXT, text.clone()),
            (html, page.clone()),
            (CF_HDROP, file.clone()),
            (CF_DIB, image.clone()),
        ]);

        let snapshot = clipboard.capture().unwrap();
        let stamp = clipboard.write_text("диктовка").unwrap();
        assert!(crate::windows_util::clipboard_contains("диктовка"));
        assert!(clipboard.still_ours(&stamp));
        clipboard.restore(&snapshot).unwrap();
        let after = clipboard.capture().unwrap();

        clipboard.restore(&users).unwrap();
        assert!(holds(&after, CF_UNICODETEXT, &text));
        assert!(holds(&after, html, &page));
        assert!(holds(&after, CF_HDROP, &file));
        assert!(holds(&after, CF_DIB, &image));
    }

    #[test]
    #[ignore = "uses the live system clipboard"]
    fn an_empty_clipboard_stays_empty_and_a_newer_copy_is_detected() {
        let clipboard = Clipboard;
        let users = clipboard.capture().unwrap();

        set(&[]);
        let snapshot = clipboard.capture().unwrap();
        let stamp = clipboard.write_text("диктовка").unwrap();
        clipboard.restore(&snapshot).unwrap();
        let after = clipboard.capture().unwrap();
        let changed = !clipboard.still_ours(&stamp);

        clipboard.restore(&users).unwrap();
        assert!(snapshot.is_empty());
        assert!(after.is_empty(), "{after:?}");
        assert!(changed, "a later write must not look like ours");
    }
}
