//! NSPasteboard: every type of every item, copied as data.
//!
//! That covers plain and rich text, HTML, images, file URLs and application
//! types, including several items (several copied files). Promised data the
//! owner has not delivered is rendered on capture; a type that still yields
//! no data fails the capture rather than being dropped.
//!
//! NSPasteboard has no lock another process would respect. `changeCount` is
//! therefore compared as the last step before every clear, after the items
//! to write are built, and a failed write's stamp is the count its own clear
//! returned. That leaves a window of a few calls rather than the wait for
//! access Windows needs; it is narrowed, not closed.

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardWriting};
use objc2_foundation::{NSArray, NSData, NSString};
use tauri::AppHandle;

use super::WriteError;

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

    fn replace_with_text(
        &mut self,
        text: &str,
        expected: &isize,
    ) -> Result<isize, WriteError<isize>> {
        let item = item_with(&[
            (PLAIN_TEXT.to_string(), text.as_bytes().to_vec()),
            (TRANSIENT.to_string(), Vec::new()),
        ])
        .map_err(untouched)?;
        self.write(vec![item], *expected)
    }

    fn replace_with(
        &mut self,
        snapshot: &Self::Snapshot,
        expected: &isize,
    ) -> Result<isize, WriteError<isize>> {
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
            .collect::<Result<Vec<_>, String>>()
            .map_err(untouched)?;
        self.write(items, *expected)
    }
}

fn untouched(error: String) -> WriteError<isize> {
    WriteError::Failed { error, left: None }
}

impl Pasteboard {
    /// Clear the pasteboard and write `items`, unless its count moved past
    /// `expected`. The check is the last step before the clear, with
    /// everything to write already prepared; it narrows the window another
    /// application could slip into, it does not close it.
    fn write(
        &self,
        items: Vec<Retained<NSPasteboardItem>>,
        expected: isize,
    ) -> Result<isize, WriteError<isize>> {
        if self.0.changeCount() != expected {
            return Err(WriteError::Changed);
        }
        // The count our own clear produced. It is the stamp of this write
        // whether or not the items go in: `writeObjects` after `clearContents`
        // keeps the owner and does not move the count, so a later read could
        // only ever be someone else's copy. Should the count move after all,
        // the restore sees a mismatch and leaves the pasteboard alone.
        let cleared = self.0.clearContents();
        // An empty pasteboard stays empty: no marker on it.
        if items.is_empty() {
            return Ok(cleared);
        }
        match write_items(&self.0, items) {
            Ok(()) => Ok(cleared),
            Err(error) => Err(WriteError::Failed {
                error,
                left: Some(cleared),
            }),
        }
    }
}

/// Native checks of what the restore guard relies on. They run against a
/// private pasteboard, never the user's, so they are not ignored: the macOS
/// CI job runs them.
#[cfg(test)]
mod tests {
    use super::super::{Preserver, Restore, Session, Store};
    use super::*;

    /// A pasteboard of its own, released when the test ends.
    struct Private(Retained<NSPasteboard>);

    impl Private {
        fn with_text(text: &str) -> Self {
            let private = Self(NSPasteboard::pasteboardWithUniqueName());
            private.copy(text);
            private
        }

        /// What another application copying `text` does.
        fn copy(&self, text: &str) {
            self.0.clearContents();
            assert!(self
                .0
                .setString_forType(&NSString::from_str(text), &NSString::from_str(PLAIN_TEXT)));
        }

        fn text(&self) -> Option<String> {
            self.0
                .stringForType(&NSString::from_str(PLAIN_TEXT))
                .map(|text| text.to_string())
        }
    }

    impl Drop for Private {
        fn drop(&mut self) {
            // SAFETY: `releaseGlobally` takes no arguments and returns void.
            unsafe {
                let _: () = objc2::msg_send![&*self.0, releaseGlobally];
            }
        }
    }

    impl Store for Private {
        type Session = Pasteboard;

        fn open(&self) -> Result<Pasteboard, String> {
            Ok(Pasteboard(self.0.clone()))
        }
    }

    #[test]
    fn a_write_keeps_the_count_its_clear_returned() {
        let private = Private::with_text("старое");
        let mut session = private.open().unwrap();
        let before = session.stamp();
        let stamp = session.replace_with_text("диктовка", &before).unwrap();
        assert_ne!(stamp, before);
        // `writeObjects` after `clearContents` must not move the count: the
        // stamp is all the restore has to recognise Sotto's write.
        assert_eq!(private.0.changeCount(), stamp);
        assert_eq!(private.text().as_deref(), Some("диктовка"));
    }

    #[test]
    fn the_users_text_comes_back_when_nothing_else_was_copied() {
        let private = Private::with_text("старое");
        let mut preserver = Preserver::new();
        let generation = preserver.begin(&private, "диктовка").unwrap();
        assert_eq!(private.text().as_deref(), Some("диктовка"));
        assert_eq!(preserver.restore(generation, &private), Restore::Restored);
        assert_eq!(private.text().as_deref(), Some("старое"));
    }

    #[test]
    fn a_copy_made_before_the_restore_survives() {
        let private = Private::with_text("старое");
        let mut preserver = Preserver::new();
        let generation = preserver.begin(&private, "диктовка").unwrap();
        private.copy("новое");
        assert_eq!(
            preserver.restore(generation, &private),
            Restore::ChangedByUser
        );
        assert_eq!(private.text().as_deref(), Some("новое"));
    }
}
