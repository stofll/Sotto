"""Render the editable DMG artwork with the existing tests/ui Playwright runtime."""

from pathlib import Path

from playwright.sync_api import sync_playwright


def main():
    artwork = Path(__file__).resolve().parents[1] / "desktop/src-tauri/installer/macos"
    source = (artwork / "background.svg").read_text(encoding="utf-8")
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch()
        for scale in (1, 2):
            page = browser.new_page(
                viewport={"width": 660, "height": 400}, device_scale_factor=scale
            )
            page.set_content(source)
            page.evaluate("document.fonts.ready")
            suffix = "" if scale == 1 else "@2x"
            page.locator("svg").screenshot(
                path=str(artwork / f"background{suffix}.png")
            )
            page.close()
        browser.close()


if __name__ == "__main__":
    main()
