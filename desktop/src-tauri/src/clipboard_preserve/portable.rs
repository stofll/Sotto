//! Clipboard through `tauri-plugin-clipboard-manager`, for the platforms
//! without a native backend (Linux). The plugin reads only plain text and
//! images, so other formats — HTML, copied files — are not kept: text stays
//! as plain text, anything unreadable is cleared after the paste.
//!
//! Known gaps, tracked separately because Linux is experimental: the reads
//! run on the thread `run_delivery` picks (the main thread off Windows),
//! which the plugin warns can freeze on content copied from our own WebView,
//! and a failed read is indistinguishable from an empty clipboard here. There
//! is no change counter either, so ownership is judged by the text: a copy of
//! exactly the dictated text made before the restore counts as Sotto's, and
//! nothing re-checks the clipboard just before it is written.

// Compiled into the tests on every platform so the Linux path cannot rot
// unnoticed; only Linux calls it.
#![cfg_attr(test, allow(dead_code))]

use tauri::image::Image;
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

use super::WriteError;

pub struct Clipboard {
    app: AppHandle,
}

impl Clipboard {
    pub fn new(app: &AppHandle) -> Self {
        Self { app: app.clone() }
    }
}

impl super::Store for Clipboard {
    type Session = Plugin;

    fn open(&self) -> Result<Plugin, String> {
        Ok(Plugin(self.app.clone()))
    }
}

pub struct Plugin(AppHandle);

pub enum Snapshot {
    Empty,
    Text(String),
    Image {
        rgba: Vec<u8>,
        width: u32,
        height: u32,
    },
}

/// The text on the clipboard. A clipboard without text never counts as
/// Sotto's: an image the user copies must not look like one Sotto restored.
pub struct Text(Option<String>);

impl PartialEq for Text {
    fn eq(&self, other: &Self) -> bool {
        matches!((&self.0, &other.0), (Some(ours), Some(theirs)) if ours == theirs)
    }
}

impl super::Session for Plugin {
    type Snapshot = Snapshot;
    type Stamp = Text;

    fn stamp(&self) -> Text {
        Text(self.0.clipboard().read_text().ok())
    }

    fn capture(&self) -> Result<Snapshot, String> {
        let clipboard = self.0.clipboard();
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

    /// `expected` is not checked here; see the module notes.
    fn replace_with_text(
        &mut self,
        text: &str,
        _expected: &Text,
    ) -> Result<Text, WriteError<Text>> {
        match crate::clipboard::copy_to_clipboard(&self.0, text) {
            Ok(()) => Ok(Text(Some(text.to_string()))),
            Err(error) => Err(WriteError::Failed {
                error,
                left: Some(self.stamp()),
            }),
        }
    }

    fn replace_with(
        &mut self,
        snapshot: &Snapshot,
        _expected: &Text,
    ) -> Result<Text, WriteError<Text>> {
        let clipboard = self.0.clipboard();
        let written = match snapshot {
            Snapshot::Empty => clipboard.clear(),
            Snapshot::Text(text) => clipboard.write_text(text.as_str()),
            Snapshot::Image {
                rgba,
                width,
                height,
            } => clipboard.write_image(&Image::new_owned(rgba.clone(), *width, *height)),
        };
        match written {
            Ok(()) => Ok(self.stamp()),
            Err(error) => Err(WriteError::Failed {
                error: error.to_string(),
                left: Some(self.stamp()),
            }),
        }
    }
}
