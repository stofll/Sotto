//! NSPasteboard: every type of every item, copied as data.
//!
//! That covers plain and rich text, HTML, images, file URLs and application
//! types, including several items (several copied files). Promised data the
//! owner has not delivered is rendered on capture.

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardWriting};
use objc2_foundation::{NSArray, NSData, NSString};
use tauri::AppHandle;

/// `NSPasteboardTypeString`.
const PLAIN_TEXT: &str = "public.utf8-plain-text";

/// The nspasteboard.org convention clipboard managers honour: content that is
/// on the pasteboard only for a moment and must not be recorded. Also set on
/// the restored copy, which they recorded when the user copied it.
const TRANSIENT: &str = "org.nspasteboard.TransientType";

pub struct Clipboard;

impl Clipboard {
    pub fn new(_app: &AppHandle) -> Self {
        Self
    }
}

fn item_with(types: &[(String, Vec<u8>)]) -> Retained<NSPasteboardItem> {
    let item = NSPasteboardItem::new();
    for (kind, bytes) in types {
        item.setData_forType(&NSData::with_bytes(bytes), &NSString::from_str(kind));
    }
    item
}

fn write_items(
    pasteboard: &NSPasteboard,
    items: Vec<Retained<NSPasteboardItem>>,
) -> Result<(), String> {
    let objects: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = items
        .into_iter()
        .map(ProtocolObject::from_retained)
        .collect();
    if pasteboard.writeObjects(&NSArray::from_retained_slice(&objects)) {
        Ok(())
    } else {
        Err("could not write to the pasteboard".into())
    }
}

impl super::Store for Clipboard {
    /// Items in pasteboard order, each with its types and their data.
    type Snapshot = Vec<Vec<(String, Vec<u8>)>>;
    /// `changeCount` right after our write.
    type Stamp = isize;

    fn capture(&self) -> Result<Self::Snapshot, String> {
        let pasteboard = NSPasteboard::generalPasteboard();
        let Some(items) = pasteboard.pasteboardItems() else {
            return Ok(Vec::new());
        };
        let mut skipped = 0;
        let snapshot = items
            .to_vec()
            .into_iter()
            .map(|item| {
                item.types()
                    .to_vec()
                    .into_iter()
                    .filter_map(|kind| match item.dataForType(&kind) {
                        Some(data) => Some((kind.to_string(), data.to_vec())),
                        None => {
                            skipped += 1;
                            None
                        }
                    })
                    .collect()
            })
            .collect();
        if skipped > 0 {
            log::warn!("{skipped} pasteboard types not saved");
        }
        Ok(snapshot)
    }

    fn write_text(&self, text: &str) -> Result<isize, String> {
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        write_items(
            &pasteboard,
            vec![item_with(&[
                (PLAIN_TEXT.to_string(), text.as_bytes().to_vec()),
                (TRANSIENT.to_string(), Vec::new()),
            ])],
        )?;
        Ok(pasteboard.changeCount())
    }

    fn still_ours(&self, stamp: &isize) -> bool {
        NSPasteboard::generalPasteboard().changeCount() == *stamp
    }

    fn restore(&self, snapshot: &Self::Snapshot) -> Result<(), String> {
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        // An empty pasteboard stays empty: no marker on it.
        if snapshot.is_empty() {
            return Ok(());
        }
        let items = snapshot
            .iter()
            .enumerate()
            .map(|(index, types)| {
                let item = item_with(types);
                if index == 0 && !types.iter().any(|(kind, _)| kind == TRANSIENT) {
                    item.setData_forType(&NSData::with_bytes(&[]), &NSString::from_str(TRANSIENT));
                }
                item
            })
            .collect();
        write_items(&pasteboard, items)
    }
}
