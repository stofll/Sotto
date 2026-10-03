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


STEPS = ["catalog", "cleanup", "profile", "overlay", "history", "file"]


def site_url(locale):
    return os.environ["SOTTO_SITE_URL"] + ("/ru/" if locale == "ru" else "/")


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("width", [390, 1440])
@pytest.mark.parametrize("appearance", ["light", "dark"])
def test_tour_catalog_and_layout(browser, locale, width, appearance):
    context = browser.new_context(
        viewport={"width": width, "height": 1000},
        device_scale_factor=2,
        color_scheme=appearance,
        reduced_motion="reduce",
    )
    page = context.new_page()
    errors = []
    page.on("pageerror", lambda error: errors.append(str(error)))
    page.goto(site_url(locale))
    page.evaluate("() => document.fonts.ready")
    tour = page.locator("[data-tour]")
    expect(tour).to_have_class(re.compile(r"\bis-enhanced\b"))
    if width >= 960:
        # The orange phrase is one line; text ranges detect wrapping directly.
        assert page.locator(".hero-title > span").evaluate(
            "el => { const r = document.createRange(); r.selectNodeContents(el); return r.getClientRects().length === 1; }"
        )
    tour.scroll_into_view_if_needed()
    sizes = []
    for index, step in enumerate(STEPS):
        tab = page.locator(f"#feature-tab-{step}")
        tab.click()
        expect(tab).to_have_attribute("aria-selected", "true")
        expect(page.get_by_role("tabpanel")).to_have_count(1)
        expect(page.get_by_role("tabpanel")).to_have_attribute("data-scene", step)
        expect(page.locator(f"#feature-{step}")).to_be_visible()
        sizes.append(page.locator(".tour-scenes").bounding_box())
        # Every step fits the page width, including the overlay's widest state.
        assert page.evaluate(
            "document.documentElement.scrollWidth <= document.documentElement.clientWidth"
        )
    # Overlapping scenes reserve the tallest one, so switching never moves the page.
    assert max(box["height"] for box in sizes) - min(box["height"] for box in sizes) < 1

    for step, region in [("catalog", "resources"), ("history", "formatting")]:
        page.locator(f"#feature-tab-{step}").click()
        scene = page.locator(f"#feature-{step}")
        image = scene.locator("[data-shot] img")
        expect(image).to_have_js_property("complete", True)
        expect(image).to_have_js_property("naturalWidth", 2176)
        detail = scene.locator(f'[data-region="{region}"]')
        detail.click()
        expect(detail).to_have_attribute("aria-pressed", "true")
        expect(scene.locator(f'[data-shot-ring="{region}"]')).to_have_class(
            re.compile(r"\bis-on\b")
        )
        assert scene.locator("[data-shot-layer]").evaluate(
            "el => el.style.transform.includes('scale')"
        )
        detail.click()
        expect(detail).to_have_attribute("aria-pressed", "false")
        assert scene.locator("[data-shot-layer]").evaluate(
            "el => el.style.transform === ''"
        )

    page.locator("#feature-tab-catalog").focus()
    page.keyboard.press("ArrowDown")
    expect(page.locator("#feature-tab-cleanup")).to_be_focused()
    expect(page.locator("#feature-tab-cleanup")).to_have_attribute(
        "aria-selected", "true"
    )
    page.keyboard.press("End")
    expect(page.locator("#feature-tab-file")).to_be_focused()
    page.keyboard.press("ArrowRight")
    expect(page.locator("#feature-tab-catalog")).to_have_attribute(
        "aria-selected", "true"
    )

    output = Path(
        os.environ.get("SOTTO_SITE_CHECKS_DIR", os.environ.get("TEMP", "/tmp"))
    )
    output.mkdir(parents=True, exist_ok=True)
    if appearance == "dark":
        page.locator("#feature-tab-overlay").click()
        page.mouse.move(0, 0)
        tour.screenshot(path=str(output / f"tour-{locale}-{width}.png"))
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
    assert (
        answer.bounding_box()["width"]
        > page.locator(".faq-item").first.bounding_box()["width"] * 0.9
    )
    assert not errors
    context.close()


@pytest.mark.parametrize("locale", ["ru", "en"])
def test_overlay_scene_switches_and_plays(browser, locale):
    context = browser.new_context(viewport={"width": 1440, "height": 1000})
    page = context.new_page()
    page.clock.install()
    page.goto(site_url(locale))
    page.locator("[data-tour]").scroll_into_view_if_needed()
    page.locator("#feature-tab-overlay").click()
    scene = page.locator("#feature-overlay")
    # Selecting the step restarts the cycle from a new recording.
    page.clock.run_for(100)
    expect(scene).to_have_attribute("data-phase", "rec")
    page.clock.run_for(2500)
    expect(scene).to_have_attribute("data-phase", "stream")

    page.locator('[data-ov-template="bead"]').click()
    expect(scene).to_have_attribute("data-template", "bead")
    expect(scene.locator('.tour-detail-value[data-for="bead"]').first).to_be_visible()
    expect(scene.locator('.tour-detail-value[data-for="pill"]').first).to_be_hidden()
    expect(scene.locator(".ov-note")).to_be_visible()
    page.locator('[data-ov-palette="lagoon"]').click()
    assert (
        scene.locator("[data-ov-stage]").evaluate(
            "el => el.style.getPropertyValue('--ov-hue')"
        )
        == "195"
    )

    # A chosen state holds instead of cycling on.
    page.locator('[data-ov-phase="proc"]').click()
    page.clock.run_for(8000)
    expect(scene).to_have_attribute("data-phase", "proc")
    expect(scene.locator(".ov-status")).to_be_visible()

    # Another step stops the animation.
    level = lambda: scene.locator("[data-ov]").evaluate(
        "el => el.style.getPropertyValue('--level')"
    )
    page.locator('[data-ov-phase="rec"]').click()
    page.clock.run_for(500)
    before = level()
    page.clock.run_for(500)
    assert level() != before
    page.locator("#feature-tab-file").click()
    page.clock.run_for(100)
    before = level()
    page.clock.run_for(1000)
    assert level() == before
    context.close()


def test_file_scene_walks_through_states(browser):
    context = browser.new_context(viewport={"width": 1440, "height": 1000})
    page = context.new_page()
    page.clock.install()
    page.goto(site_url("ru"))
    page.locator("[data-tour]").scroll_into_view_if_needed()
    scene = page.locator("#feature-file")
    expect(scene).to_have_attribute("data-state", "done")
    page.locator("#feature-tab-file").click()
    expect(scene).to_have_attribute("data-state", "idle")
    page.clock.run_for(1500)
    expect(scene).to_have_attribute("data-state", "reading")
    page.clock.run_for(1200)
    expect(scene).to_have_attribute("data-state", "transcribing")
    page.clock.run_for(2700)
    expect(scene).to_have_attribute("data-state", "done")
    expect(scene.locator(".file-result")).to_be_visible()
    context.close()


@pytest.mark.parametrize("locale", ["ru", "en"])
def test_without_javascript(browser, locale):
    context = browser.new_context(java_script_enabled=False)
    page = context.new_page()
    page.goto(site_url(locale))
    expect(page.locator(".tour-scene:visible")).to_have_count(len(STEPS))
    expect(page.locator(".tour-steps")).to_be_hidden()
    expect(page.locator("#feature-overlay .ov-draft")).to_be_visible()
    expect(page.locator("#feature-file .file-result")).to_be_visible()
    page.locator(".model-catalog summary").click()
    expect(page.locator(".model-card:visible")).to_have_count(
        page.locator(".model-card").count()
    )
    context.close()
