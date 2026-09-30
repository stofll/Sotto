"""Website interactions against an explicitly selected local dev/preview server."""

import os
import re
from pathlib import Path

import pytest
from playwright.sync_api import expect

pytestmark = pytest.mark.skipif(
    "SOTTO_SITE_URL" not in os.environ,
    reason="set SOTTO_SITE_URL to a running local website",
)


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("width", [390, 1440])
@pytest.mark.parametrize("appearance", ["light", "dark"])
def test_gallery_catalog_and_layout(browser, locale, width, appearance):
    context = browser.new_context(
        viewport={"width": width, "height": 1000},
        device_scale_factor=2,
        color_scheme=appearance,
        reduced_motion="reduce",
    )
    page = context.new_page()
    errors = []
    page.on("pageerror", lambda error: errors.append(str(error)))
    page.goto(os.environ["SOTTO_SITE_URL"] + ("/ru/" if locale == "ru" else "/"))
    page.evaluate("() => document.fonts.ready")
    gallery = page.locator("[data-screens]")
    expect(gallery).to_have_class(re.compile(r"\bis-enhanced\b"))
    assert page.evaluate(
        "document.documentElement.scrollWidth <= document.documentElement.clientWidth"
    )
    if width >= 960:
        # The orange phrase is one line; text ranges detect wrapping directly.
        assert page.locator(".hero-title > span").evaluate(
            "el => { const r = document.createRange(); r.selectNodeContents(el); return r.getClientRects().length === 1; }"
        )
    page.clock.install()
    gallery.scroll_into_view_if_needed()
    page.clock.run_for(12000)
    expect(page.get_by_role("tab").nth(0)).to_have_attribute("aria-selected", "true")
    sizes = []
    for index in range(3):
        tab = page.get_by_role("tab").nth(index)
        tab.click()
        expect(tab).to_have_attribute("aria-selected", "true")
        panel = page.get_by_role("tabpanel")
        expect(panel).to_be_visible()
        img = panel.locator('[data-theme-image="dark"]')
        expect(img).to_have_js_property("complete", True)
        expect(img).to_have_js_property("naturalWidth", 1088)
        sizes.append(gallery.bounding_box())
        closer = page.locator("[data-screen-zoom]")
        closer.click()
        expect(page.get_by_role("dialog")).to_be_visible()
        expect(page.locator("[data-screen-full]")).to_have_attribute(
            "src", img.evaluate("el => new URL(el.dataset.fullSrc, location.href).href")
        )
        page.keyboard.press("Escape")
        expect(page.get_by_role("dialog")).not_to_be_visible()
        expect(closer).to_be_focused()
    assert max(box["height"] for box in sizes) - min(box["height"] for box in sizes) < 1
    assert max(box["width"] for box in sizes) - min(box["width"] for box in sizes) < 1
    page.get_by_role("tab").nth(2).focus()
    page.keyboard.press("ArrowRight")
    expect(page.get_by_role("tab").nth(0)).to_be_focused()
    page.locator("[data-tour-toggle]").click()
    expect(page.locator('[data-step-description="shortcut"]')).to_be_visible()
    page.clock.run_for(6600)
    expect(page.locator('[data-step-description="recording"]')).to_be_visible()
    expect(page.get_by_role("tab").nth(0)).to_have_attribute("aria-selected", "true")
    expect(page.locator('.screen-hotspot[data-step-target="recording"]')).to_have_attribute("aria-pressed", "true")
    page.locator('[data-tour-next]').click()
    expect(page.locator('[data-step-description="microphone"]')).to_be_visible()
    page.locator('[data-tour-next]').click()
    expect(page.get_by_role("tab").nth(1)).to_have_attribute("aria-selected", "true")
    page.locator('[data-theme-target="light"]').click()
    expect(gallery).to_have_attribute("data-theme", "light")
    expect(page.locator('[data-step-description="languages"]')).to_be_visible()
    light_image = page.get_by_role("tabpanel").locator('[data-theme-image="light"]')
    expect(light_image).to_have_css("opacity", "1")
    expect(light_image).to_have_js_property("complete", True)
    page.locator('[data-screen-zoom]').click()
    expect(page.locator('[data-screen-full]')).to_have_attribute("src", light_image.evaluate("el => new URL(el.dataset.fullSrc, location.href).href"))
    page.keyboard.press("Escape")
    page.locator('[data-theme-target="dark"]').click()
    expect(gallery).to_have_attribute("data-theme", "dark")
    page.get_by_role("tab").nth(0).click()
    expect(page.locator("[data-tour-toggle]")).to_have_attribute(
        "aria-pressed", "false"
    )

    output = Path(
        os.environ.get("SOTTO_SITE_CHECKS_DIR", os.environ.get("TEMP", "/tmp"))
    )
    output.mkdir(parents=True, exist_ok=True)
    if appearance == "dark":
        page.clock.resume()
        page.mouse.move(0, 0)
        gallery.locator("..").screenshot(path=str(output / f"gallery-{locale}-{width}.png"))
        page.screenshot(path=str(output / f"site-{locale}-{width}.png"), full_page=True)

    catalog = page.locator(".model-catalog")
    expect(catalog).not_to_have_attribute("open", "")
    expect(page.locator(".model-pick")).to_have_count(4)
    catalog.locator("summary").click()
    total = page.locator(".model-card").count()
    assert total > 4
    expect(page.locator(".model-card:visible")).to_have_count(total)
    page.locator('[data-filter="ru"]').click()
    assert page.locator(".model-card:visible").count() < total
    for card in page.locator(".model-card:visible").all():
        assert "ru" in card.get_attribute("data-groups").split()
    page.locator("[data-streaming-filter]").click()
    for card in page.locator(".model-card:visible").all():
        assert card.get_attribute("data-streaming") is not None
    page.locator('[data-filter="all"]').click()
    page.locator("[data-streaming-filter]").click()
    expect(page.locator(".model-card:visible")).to_have_count(total)
    expect(page.locator("section#privacy")).to_have_count(0)
    expect(page.locator('.desktop-nav a[href$="/privacy/"]')).to_have_count(1)
    expect(page.locator(".voice-label")).to_have_count(0)
    page.locator(".faq-item summary").first.click()
    answer = page.locator(".faq-item p").first
    assert answer.bounding_box()["width"] > page.locator(".faq-item").first.bounding_box()["width"] * 0.9
    assert not errors
    context.close()


@pytest.mark.parametrize("locale", ["ru", "en"])
def test_feature_steps_and_crossfade(browser, locale):
    context = browser.new_context(viewport={"width": 1440, "height": 1200}, device_scale_factor=1)
    page = context.new_page()
    light_requests = []
    page.on("request", lambda request: light_requests.append(request.url) if re.search(r"-light[.@]", request.url) else None)
    page.goto(os.environ["SOTTO_SITE_URL"] + ("/ru/" if locale == "ru" else "/"))
    gallery = page.locator("[data-screens]")
    gallery.scroll_into_view_if_needed()
    expect(gallery).to_have_attribute("data-theme", "dark")
    steps = ["shortcut", "recording", "microphone", "languages", "resources", "streaming", "search", "copy", "formatting"]
    heights = []
    for index, step in enumerate(steps):
        expect(page.locator(f'[data-step-description="{step}"]')).to_be_visible()
        expect(page.locator(f'.screen-hotspot[data-step-target="{step}"]')).to_have_attribute("aria-pressed", "true")
        heights.append(page.locator('[data-tour-guide]').bounding_box()["height"])
        image = page.get_by_role("tabpanel").locator('[data-theme-image="dark"]')
        expect(image).to_have_js_property("complete", True)
        assert "@2x" not in image.evaluate("el => el.currentSrc")
        if index < len(steps) - 1:
            page.locator('[data-tour-next]').click()
    expect(page.locator('[data-tour-next]')).to_be_disabled()
    assert max(heights) - min(heights) < 1
    # Hold the screen transitions as they start and step through them, so a slow
    # runner cannot skip past the fade: no point may hide both screens.
    page.evaluate("""() => {
        window.holdFade = true;
        document.querySelectorAll('.screen-window').forEach(el => el.addEventListener('transitionrun', () => {
            if (window.holdFade) el.getAnimations().forEach(animation => animation.pause());
        }));
    }""")
    page.get_by_role("tab").nth(0).click()
    expect(page.locator('[data-step-description="shortcut"]')).to_be_visible()
    frames = page.evaluate("""() => {
        const windows = [...document.querySelectorAll('.screen-window')];
        const frames = [0, 60, 120, 180, 240, 300, 355].map(time => windows.map(el => {
            el.getAnimations().forEach(animation => { animation.currentTime = time; });
            const s = getComputedStyle(el); return s.visibility === 'visible' ? Number(s.opacity) : 0;
        }));
        window.holdFade = false;
        windows.forEach(el => el.getAnimations().forEach(animation => animation.finish()));
        return frames;
    }""")
    assert all(max(frame) > 0.25 for frame in frames)
    assert any(sum(value > 0.05 for value in frame) > 1 for frame in frames)
    # The whole dark tour ran without fetching a light capture.
    assert light_requests == []
    page.locator('[data-theme-target="light"]').click()
    expect(gallery).to_have_attribute("data-theme", "light")
    assert light_requests
    page.locator('.screen-hotspot[data-step-target="recording"]').click()
    expect(page.locator('[data-step-description="recording"]')).to_be_visible()
    expect(gallery).to_have_attribute("data-theme", "light")
    context.close()


def test_failed_image_keeps_current_screen_and_can_retry(browser):
    context = browser.new_context()
    page = context.new_page()
    page.route("**/*settings-ru-light*.webp*", lambda route: route.abort())
    page.goto(os.environ["SOTTO_SITE_URL"] + "/ru/")
    page.locator('[data-theme-target="light"]').click()
    expect(page.locator('[data-screen-error]')).to_be_visible()
    expect(page.locator('[data-screens]')).to_have_attribute("data-theme", "dark")
    page.unroute("**/*settings-ru-light*.webp*")
    page.locator('[data-theme-target="light"]').click()
    expect(page.locator('[data-screens]')).to_have_attribute("data-theme", "light")
    expect(page.locator('[data-screen-error]')).not_to_be_visible()
    context.close()


@pytest.mark.parametrize("locale", ["ru", "en"])
def test_without_javascript(browser, locale):
    context = browser.new_context(java_script_enabled=False)
    page = context.new_page()
    page.goto(os.environ["SOTTO_SITE_URL"] + ("/ru/" if locale == "ru" else "/"))
    expect(page.locator(".screen-window:visible")).to_have_count(3)
    page.locator(".model-catalog summary").click()
    expect(page.locator(".model-card:visible")).to_have_count(page.locator(".model-card").count())
    expect(page.locator(".screen-fallback a").first).to_have_attribute(
        "href", page.locator(".screen-image img").first.get_attribute("data-full-src")
    )
    context.close()
