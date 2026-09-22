# Installer design

The Windows installer uses a separate setup application so installation UI and animation do not enter the dictation, overlay or tray startup paths. The welcome screen has the application name, one installation button and quiet animated background waves. The original icon appears only in the native title bar. Theme and language controls sit at the top right, the version at the bottom right, and installation details open in a dismissible popover without shifting the layout. Respect reduced motion and stop decorative animation while the window is hidden.

The setup application embeds the existing NSIS package. That package remains responsible for application files, native libraries, shortcuts, migration and uninstall registration. The updater continues to consume the original signed NSIS artifact; the visual setup executable is a separate download.

Use a large Sotto wordmark with the subtitle «Мысли становятся текстом.». Installation options sit below the primary button and open a separate view in the same window. Keep the folder and two shortcut choices out of the welcome screen; animate opening and Back without changing window height or adding page scrolling. Show the actual Windows-resolved folder instead of environment-variable notation, and preserve choices when navigating back.

## macOS DMG

Keep the standard drag to Applications workflow. The background is light: quiet waves, a dark Sotto heading, and four orange chevrons that brighten towards the Applications folder. The original Sotto icon sits on the left and the system Applications folder on the right. Finder always draws icon labels in black, whatever the background or system appearance, so a dark background would leave the labels unreadable. The background has no installation instruction text, so it needs no language-specific variants.

The editable artwork is `desktop/src-tauri/installer/macos/background.svg`. Run `uv run --locked --project tests/ui python scripts/render-dmg-background.py` from the repository root to regenerate `background.png` and `background@2x.png` using the existing Playwright Chromium installation. The chevrons and the icon positions in `scripts/macos-dmg/dmg_settings.py` share one centre line; change them together.

The disk image is built by `scripts/build-dmg.sh` with dmgbuild, pinned in `scripts/macos-dmg/uv.lock`, rather than by the Tauri bundler. The bundler lays out the window by driving Finder through AppleScript: on macOS 26 Finder kept the icon positions but dropped the background and icon size, and a headless CI runner cannot drive Finder at all. dmgbuild writes the window settings into `.DS_Store` itself. The script combines both PNGs into a multi-resolution TIFF so Retina displays get the @2x artwork, attaches the license, and signs the image when `APPLE_SIGNING_IDENTITY` is set.

Do not hide the `.app` extension through dmgbuild: it sets Finder information on the bundle, and `codesign --verify --strict` then rejects the application. Before publishing, open the release DMG on a Mac and check the layout in both system appearances.
