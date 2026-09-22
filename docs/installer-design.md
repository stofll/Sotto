# Installer design

The Windows installer uses a separate setup application so installation UI and animation do not enter the dictation, overlay or tray startup paths. The welcome screen has the application name, one installation button and quiet animated background waves. The original icon appears only in the native title bar. Theme and language controls sit at the top right, the version at the bottom right, and installation details open in a dismissible popover without shifting the layout. Respect reduced motion and stop decorative animation while the window is hidden.

The setup application embeds the existing NSIS package. That package remains responsible for application files, native libraries, shortcuts, migration and uninstall registration. The updater continues to consume the original signed NSIS artifact; the visual setup executable is a separate download.

Use a large Sotto wordmark with the subtitle «Мысли становятся текстом.». Installation options sit below the primary button and open a separate view in the same window. Keep the folder and two shortcut choices out of the welcome screen; animate opening and Back without changing window height or adding page scrolling. Show the actual Windows-resolved folder instead of environment-variable notation, and preserve choices when navigating back.

## macOS DMG

Keep the standard drag to Applications workflow. The chosen design uses a fixed dark background with quiet waves and a light Sotto heading, the original Sotto icon on the left and the system Applications folder on the right. An orange arrow composed of small right-pointing arrows indicates dragging. The background has no installation instruction text, so it does not need language-specific variants; text embedded in a DMG background would not translate automatically. The background does not switch with the system appearance.

The editable artwork is desktop/src-tauri/installer/macos/background.svg. Run `uv run --locked --project tests/ui python scripts/render-dmg-background.py` from the repository root to regenerate its PNG assets using the existing Playwright Chromium installation. The macOS-only Tauri configuration selects the 660×400 PNG and matching icon positions. The @2x asset is available for Retina packaging work; native scaling must be checked in Finder before selecting it as the background. Icons and their names are real Finder items, not baked into the artwork.

Release CI sets TAURI_BUNDLER_DMG_IGNORE_CI=true on macOS so Tauri runs the Finder layout step instead of skipping background and position customization. This requires a working graphical Finder session on the macOS runner. Verify the resulting disk image rather than treating a successful archive build as proof of its layout.

- [ ] Verify layout, icon label contrast in both system appearances and Retina scaling in Finder on a Mac.
- [ ] Verify the release runner applies the background and positions without hanging on Finder automation.
- [ ] Verify signing, notarization, drag-to-Applications installation and first launch on macOS.
