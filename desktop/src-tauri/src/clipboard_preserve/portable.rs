//! Clipboard through `tauri-plugin-clipboard-manager`, for the platforms
//! without a native backend (Linux). The plugin reads only plain text and
//! images, so other formats — HTML, copied files — are not kept: text stays
//! as plain text, anything unreadable is cleared after the paste.

// Compiled into the tests on every platform so the Linux path cannot rot
// unnoticed; only Linux calls it.
#![cfg_attr(test, allow(dead_code))]

use tauri::image::Image;
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

pub struct Clipboard {
    app: AppHandle,
}

impl Clipboard {
    pub fn new(app: &AppHandle) -> Self {
        Self { app: app.clone() }
    }
}

pub enum Snapshot {
    Empty,
    Text(String),
    Image {
        rgba: Vec<u8>,
        width: u32,
        height: u32,
    },
}

impl super::Store for Clipboard {
    type Snapshot = Snapshot;
    /// Our text: there is no change counter to compare.
    type Stamp = String;

    fn capture(&self) -> Result<Snapshot, String> {
        let clipboard = self.app.clipboard();
        if let Ok(text) = clipboard.read_text() {
            return Ok(Snapshot::Text(text));
        }
        Ok(match clipboard.read_image() {
            Ok(image) => Snapshot::Image {
                rgba: image.rgba().to_vec(),
                width: image.width(),
                height: image.height(),
            },
            Err(_) => Snapshot::Empty,
        })
    }

    fn write_text(&self, text: &str) -> Result<String, String> {
        crate::clipboard::copy_to_clipboard(&self.app, text)?;
        Ok(text.to_string())
    }

    fn still_ours(&self, stamp: &String) -> bool {
        self.app
            .clipboard()
            .read_text()
            .is_ok_and(|text| text == *stamp)
    }

    fn restore(&self, snapshot: &Snapshot) -> Result<(), String> {
        let clipboard = self.app.clipboard();
        match snapshot {
            Snapshot::Empty => clipboard.clear(),
            Snapshot::Text(text) => clipboard.write_text(text.as_str()),
            Snapshot::Image {
                rgba,
                width,
                height,
            } => clipboard.write_image(&Image::new_owned(rgba.clone(), *width, *height)),
        }
        .map_err(|error| error.to_string())
    }
}
