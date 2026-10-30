//! Window geometry of the notch shell, the one layout whose size depends on
//! the screen (docs/overlay-customization.md, "Sizes stay named").
//!
//! On a Mac with a camera notch the band is as tall as the notch and as wide
//! as the notch plus an ear on each side. Everywhere else the named size in
//! `overlay_preferences.rs` is used and the band draws its own centre.

use serde::Serialize;

/// The hardware cut-out of one screen, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenNotch {
    /// `safeAreaInsets.top`: how far the cut-out reaches down.
    pub height: f64,
    /// Screen width minus the menu bar areas either side of the cut-out.
    pub width: f64,
}

/// What the overlay window shows of the notch, sent to the scene so its ears sit
/// either side of the cut-out. The scene draws the black centre itself when
/// there is none.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct NotchFit {
    pub width: f64,
    pub height: f64,
}

/// Mirrors `NOTCH_EAR` in overlayRecipe.ts: the width of each ear by overlay size.
pub fn ear_width(size: &str) -> f64 {
    match size {
        "s" => 80.0,
        "l" => 100.0,
        _ => 88.0,
    }
}

/// A real cut-out is a few hundred points wide at most; anything else is a
/// geometry the system reported oddly, and the named size is the safer window.
const MAX_NOTCH_WIDTH: f64 = 600.0;
const MAX_NOTCH_HEIGHT: f64 = 120.0;

/// The window of the notch shell: `named` is the size used without a notch.
/// The notch width is rounded up to an even number of points so the window
/// stays centred on the cut-out; the extra half point hides under it.
pub fn notch_window(
    named: (f64, f64),
    ear: f64,
    screen: Option<ScreenNotch>,
) -> ((f64, f64), Option<NotchFit>) {
    let usable = screen.filter(|notch| {
        notch.width.is_finite()
            && notch.height.is_finite()
            && (1.0..=MAX_NOTCH_WIDTH).contains(&notch.width)
            && (1.0..=MAX_NOTCH_HEIGHT).contains(&notch.height)
    });
    match usable {
        Some(notch) => {
            let fit = NotchFit {
                width: (notch.width / 2.0).ceil() * 2.0,
                height: notch.height.ceil(),
            };
            ((fit.width + 2.0 * ear, fit.height), Some(fit))
        }
        None => (named, None),
    }
}

/// Top-centre of the monitor, flush with its top edge, whatever the anchor says.
pub fn notch_origin(
    monitor_pos: (i32, i32),
    monitor_size: (u32, u32),
    window_width: u32,
) -> (i32, i32) {
    let spare = (i64::from(monitor_size.0) - i64::from(window_width)).max(0);
    (monitor_pos.0 + (spare / 2) as i32, monitor_pos.1)
}

/// The cut-out of the screen whose top-left corner (in points, y down) is
/// `origin`, or `None` when it has none, the system is older than macOS 12, or
/// no screen sits there. Reads AppKit, so it runs on the main thread.
#[cfg(target_os = "macos")]
pub fn read_screen_notch(origin: (f64, f64)) -> Option<ScreenNotch> {
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::{msg_send, sel};
    use objc2_foundation::{NSEdgeInsets, NSRect};

    let class = AnyClass::get(c"NSScreen")?;
    // SAFETY: AppKit calls on the main thread (the caller's contract) with the
    // return types NSScreen declares; every object comes from `screens` and is
    // not retained past this function.
    unsafe {
        let screens: *mut AnyObject = msg_send![class, screens];
        if screens.is_null() {
            return None;
        }
        let count: usize = msg_send![screens, count];
        // While displays reconfigure the list can be empty, and `objectAtIndex:`
        // past its end raises an Objective-C exception that aborts the process.
        if count == 0 {
            return None;
        }
        let frame_of = |screen: *mut AnyObject| -> NSRect { msg_send![screen, frame] };
        // AppKit's y grows upwards from the primary screen's bottom edge; the
        // monitor origin the window code uses grows downwards from its top.
        let primary: *mut AnyObject = msg_send![screens, objectAtIndex: 0usize];
        let primary_height = frame_of(primary).size.height;
        for index in 0..count {
            let screen: *mut AnyObject = msg_send![screens, objectAtIndex: index];
            let frame = frame_of(screen);
            let top = primary_height - (frame.origin.y + frame.size.height);
            if (frame.origin.x - origin.0).abs() > 1.0 || (top - origin.1).abs() > 1.0 {
                continue;
            }
            let responds: bool = msg_send![screen, respondsToSelector: sel!(safeAreaInsets)];
            let has_areas: bool = msg_send![screen, respondsToSelector: sel!(auxiliaryTopLeftArea)];
            if !responds || !has_areas {
                return None;
            }
            let insets: NSEdgeInsets = msg_send![screen, safeAreaInsets];
            if insets.top <= 0.0 {
                return None;
            }
            let left: NSRect = msg_send![screen, auxiliaryTopLeftArea];
            let right: NSRect = msg_send![screen, auxiliaryTopRightArea];
            return Some(ScreenNotch {
                height: insets.top,
                width: frame.size.width - left.size.width - right.size.width,
            });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAMED: (f64, f64) = (356.0, 32.0);

    #[test]
    fn a_notch_sets_the_height_and_the_width_of_the_window() {
        let screen = ScreenNotch {
            height: 38.0,
            width: 180.0,
        };
        let (size, fit) = notch_window(NAMED, 88.0, Some(screen));
        assert_eq!(size, (180.0 + 176.0, 38.0));
        assert_eq!(
            fit,
            Some(NotchFit {
                width: 180.0,
                height: 38.0
            })
        );
    }

    #[test]
    fn no_notch_keeps_the_named_size() {
        assert_eq!(notch_window(NAMED, 88.0, None), (NAMED, None));
    }

    #[test]
    fn odd_widths_round_up_to_keep_the_window_centred() {
        let (size, fit) = notch_window(
            NAMED,
            88.0,
            Some(ScreenNotch {
                height: 37.4,
                width: 185.0,
            }),
        );
        assert_eq!(fit.unwrap().width, 186.0);
        assert_eq!(size, (186.0 + 176.0, 38.0));
        assert_eq!(size.0 % 2.0, 0.0);
        let (_, fit) = notch_window(
            NAMED,
            88.0,
            Some(ScreenNotch {
                height: 38.0,
                width: 184.2,
            }),
        );
        assert_eq!(fit.unwrap().width, 186.0);
    }

    #[test]
    fn a_degenerate_geometry_falls_back_to_the_named_size() {
        for (height, width) in [
            (38.0, -12.0),
            (38.0, 0.0),
            (0.0, 180.0),
            (-4.0, 180.0),
            (38.0, f64::NAN),
            (f64::INFINITY, 180.0),
            (38.0, 1400.0),
            (400.0, 180.0),
        ] {
            let screen = ScreenNotch { height, width };
            assert_eq!(
                notch_window(NAMED, 88.0, Some(screen)),
                (NAMED, None),
                "{height} x {width}"
            );
        }
    }

    #[test]
    fn the_window_sits_top_centre_whatever_the_anchor() {
        assert_eq!(notch_origin((0, 0), (3024, 1964), 362 * 2), (1150, 0));
        // A monitor to the left of the primary one has a negative origin.
        assert_eq!(
            notch_origin((-1920, 0), (1920, 1080), 712),
            (-1920 + 604, 0)
        );
        // A monitor whose origin is below the primary one keeps the top edge.
        assert_eq!(notch_origin((0, 1080), (1920, 1080), 712), (604, 1080));
        // Wider than the monitor pins to its left edge.
        assert_eq!(notch_origin((10, 0), (300, 100), 400), (10, 0));
    }
}
