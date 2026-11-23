//! NSPasteboard: every type of every item, copied as data.
//!
//! That covers plain and rich text, HTML, images, file URLs and application
//! types, including several items (several copied files). Promised data the
//! owner has not delivered is rendered on capture; a type that still yields
//! no data fails the capture rather than being dropped.
//!
//! NSPasteboard has no lock another process would respect. A session is
//! therefore a sequence with no waiting in it: `changeCount` is compared
//! right before the write it guards, which leaves a window of a few calls
//! rather than the wait for access that Windows needs.

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

impl super::Store for Clipboard {
    type Session = Pasteboard;

    fn open(&self) -> Result<Pasteboard, String> {
        Ok(Pasteboard(NSPasteboard::generalPasteboard()))
    }
}

pub struct Pasteboard(Retained<NSPasteboard>);

fn item_with(types: &[(String, Vec<u8>)]) -> Result<Retained<NSPasteboardItem>, String> {
    let item = NSPasteboardItem::new();
    for (kind, bytes) in types {
        if !item.setData_forType(&NSData::with_bytes(bytes), &NSString::from_str(kind)) {
            return Err(format!("pasteboard type {kind} could not be set"));
        }
    }
    Ok(item)
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

impl super::Session for Pasteboard {
    /// Items in pasteboard order, each with its types and their data.
    type Snapshot = Vec<Vec<(String, Vec<u8>)>>;
    type Stamp = isize;

    fn stamp(&self) -> isize {
        self.0.changeCount()
    }

    fn capture(&self) -> Result<Self::Snapshot, String> {
        // An empty pasteboard is an empty array; nil means the items could
        // not be read.
        let items = self
            .0
            .pasteboardItems()
            .ok_or("the pasteboard items could not be read")?;
        items
            .to_vec()
            .into_iter()
            .map(|item| {
                item.types()
                    .to_vec()
                    .into_iter()
                    .map(|kind| match item.dataForType(&kind) {
                        Some(data) => Ok((kind.to_string(), data.to_vec())),
                        None => Err(format!("pasteboard type {kind} could not be read")),
                    })
                    .collect()
            })
            .collect()
    }

    fn replace_with_text(&mut self, text: &str) -> Result<(), String> {
        let item = item_with(&[
            (PLAIN_TEXT.to_string(), text.as_bytes().to_vec()),
            (TRANSIENT.to_string(), Vec::new()),
        ])?;
        self.0.clearContents();
        write_items(&self.0, vec![item])
    }

    fn replace_with(&mut self, snapshot: &Self::Snapshot) -> Result<(), String> {
        // Built before the pasteboard is cleared: a failure here leaves it as
        // it was.
        let items = snapshot
            .iter()
            .enumerate()
            .map(|(index, types)| {
                let item = item_with(types)?;
                if index == 0 && !types.iter().any(|(kind, _)| kind == TRANSIENT) {
                    item.setData_forType(&NSData::with_bytes(&[]), &NSString::from_str(TRANSIENT));
                }
                Ok(item)
            })
            .collect::<Result<Vec<_>, String>>()?;
        self.0.clearContents();
        // An empty pasteboard stays empty: no marker on it.
        if items.is_empty() {
            return Ok(());
        }
        write_items(&self.0, items)
    }
}
