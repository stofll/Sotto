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


FEATURES = ["voice", "cleanup", "models", "profile", "history", "file"]
SECTIONS = ["[data-desk]", "[data-closeup]", "[data-caps]", ".facts", "[data-apps]"]


def site_url(locale):
    return os.environ["SOTTO_SITE_URL"] + ("/ru/" if locale == "ru" else "/")


def run_until(page, condition, step=100, limit=30000):
    """Advances the fake clock until `condition()` holds; phase lengths depend on the locale's words."""
    for _ in range(limit // step):
        if condition():
            return
        page.clock.run_for(step)
    assert condition()


def fits_width(page):
    return page.evaluate(
        "document.documentElement.scrollWidth <= document.documentElement.clientWidth"
    )


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("width", [390, 1440])
@pytest.mark.parametrize("appearance", ["light", "dark"])
def test_layout_features_and_catalog(browser, locale, width, appearance):
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
    if width >= 960:
        # The second line of the headline is one line; text ranges detect wrapping directly.
        assert page.locator(".hero-title > span").evaluate(
            "el => { const r = document.createRange(); r.selectNodeContents(el); return r.getClientRects().length === 1; }"
        )
    for selector in SECTIONS:
        page.locator(selector).scroll_into_view_if_needed()
        assert fits_width(page), selector

    caps = page.locator("[data-caps]")
    expect(caps).to_have_class(re.compile(r"\bis-enhanced\b"))
    caps.scroll_into_view_if_needed()
    heights = []
    for feature in FEATURES:
        tab = page.locator(f"#cap-tab-{feature}")
        tab.click()
        expect(tab).to_have_attribute("aria-selected", "true")
        expect(caps.get_by_role("tabpanel")).to_have_count(1)
        expect(caps.get_by_role("tabpanel")).to_have_attribute("data-cap", feature)
        heights.append(caps.bounding_box()["height"])
        assert fits_width(page), feature
    # Panels overlap in one cell, so switching never moves the page.
    assert max(heights) - min(heights) < 1
    page.locator("#cap-tab-voice").click()
    page.keyboard.press("ArrowDown")
    expect(page.locator("#cap-tab-cleanup")).to_be_focused()
    expect(page.locator("#cap-tab-cleanup")).to_have_attribute("aria-selected", "true")
    page.keyboard.press("End")
    expect(page.locator("#cap-tab-file")).to_be_focused()
    page.keyboard.press("ArrowRight")
    expect(page.locator("#cap-tab-voice")).to_have_attribute("aria-selected", "true")

    output = Path(
        os.environ.get("SOTTO_SITE_CHECKS_DIR", os.environ.get("TEMP", "/tmp"))
    )
    output.mkdir(parents=True, exist_ok=True)
    page.screenshot(
        path=str(output / f"site-{locale}-{width}-{appearance}.png"), full_page=True
    )

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
    expect(page.locator('.desktop-nav a[href$="/privacy/"]')).to_have_count(1)
    page.locator(".faq-item summary").first.click()
    answer = page.locator(".faq-item p").first
    assert (
        answer.bounding_box()["width"]
        > page.locator(".faq-item").first.bounding_box()["width"] * 0.9
    )
    assert not errors
    context.close()


@pytest.mark.parametrize("locale", ["ru", "en"])
def test_desktop_demo_dictates_into_the_messenger(browser, locale):
    context = browser.new_context(viewport={"width": 1440, "height": 1000})
    page = context.new_page()
    # The stand-in pill: what the desk shows until, or unless, the app's overlay loads.
    page.route("**/island*", lambda route: route.abort())
    page.clock.install()
    page.goto(site_url(locale))
    desk = page.locator("[data-desk]")
    field = desk.locator("[data-desk-text]")
    overlay = desk.locator("[data-ov]")
    # Coming into view starts a dictation from an empty field, with the first caption.
    page.clock.run_for(100)
    expect(desk).to_have_attribute("data-phase", "idle")
    expect(desk).to_have_attribute("data-step", "1")
    expect(field).to_have_text("")
    expect(desk.locator(".desk-placeholder")).to_be_visible()
    page.clock.run_for(1300)
    expect(desk).to_have_attribute("data-phase", "rec")
    expect(desk).to_have_attribute("data-step", "2")
    page.clock.run_for(1000)
    expect(desk).to_have_attribute("data-phase", "stream")
    page.clock.run_for(1200)
    assert desk.locator("[data-ov-draft]").inner_text()
    run_until(page, lambda: desk.get_attribute("data-phase") == "proc")
    expect(overlay.locator(".ov-proc")).to_be_visible()
    page.clock.run_for(1100)
    expect(desk).to_have_attribute("data-phase", "done")
    expect(field).not_to_have_text("")
    expect(overlay.locator(".ov-done")).to_be_visible()
    pasted = field.inner_text()
    expect(overlay.locator("[data-ov-count]")).to_have_text(str(len(pasted)))

    # Scrolled away, the desktop holds the moment after the paste.
    page.locator("#faq").scroll_into_view_if_needed()
    page.clock.run_for(100)
    expect(desk).to_have_attribute("data-phase", "done")
    expect(field).to_have_text(pasted)
    expect(page.locator("[data-desk-tune]")).to_be_hidden()
    context.close()


@pytest.mark.parametrize("locale", ["ru", "en"])
def test_desktop_demo_shows_the_apps_overlay_looks(browser, locale):
    context = browser.new_context(viewport={"width": 1440, "height": 1000})
    page = context.new_page()
    errors = []
    page.on("pageerror", lambda error: errors.append(str(error)))
    page.clock.install()
    page.goto(site_url(locale))
    desk = page.locator("[data-desk]")
    field = desk.locator("[data-desk-text]")
    overlay = desk.locator(".desk-ovs .ovs")
    tune = page.locator("[data-desk-tune]")
    phase = lambda name: lambda: desk.get_attribute("data-phase") == name

    # The app's own overlay loads with the desk in view and takes the stand-in's place.
    run_until(page, lambda: "has-island" in (desk.get_attribute("class") or ""))
    expect(tune).to_be_visible()
    expect(desk.locator(":scope > .ov")).to_be_hidden()

    # A whole dictation: words while speaking, processing, then the pasted count.
    run_until(page, phase("idle"))
    run_until(page, phase("stream"))
    run_until(
        page,
        lambda: (
            overlay.locator(".ovs-draft").count() > 0
            and overlay.locator(".ovs-draft").inner_text() != ""
        ),
    )
    run_until(page, phase("proc"))
    expect(overlay).to_have_attribute("data-phase", "processing")
    run_until(page, phase("done"))
    expect(overlay).to_have_attribute("data-phase", "pasted")
    expect(overlay.locator(".ovs-lbl--ok")).to_contain_text(
        str(len(field.inner_text()))
    )

    # Untouched, each dictation takes the next look, and the one on screen is marked.
    first = overlay.get_attribute("data-shell")
    run_until(page, phase("rest"))
    run_until(page, phase("rec"))
    assert overlay.get_attribute("data-shell") != first
    expect(tune.locator("[data-live]")).to_have_count(1)

    # A pick holds its look and starts over; "after" keeps the draft closed.
    tune.locator('[data-look="term"]').click()
    expect(tune.locator('[data-look="term"]')).to_have_attribute("aria-pressed", "true")
    expect(tune.locator("[data-live]")).to_have_count(0)
    tune.locator('[data-streaming="false"]').click()
    run_until(page, phase("stream"))
    page.clock.run_for(1200)
    expect(overlay).to_have_attribute("data-draft", "0")
    expect(overlay.locator(".ovs-ascii")).to_have_count(1)

    # The palette recolours the overlay in place.
    tune.locator('[data-palette="violet"]').click()
    assert "295" in desk.locator(".desk-ovs-tone").evaluate(
        "el => getComputedStyle(el).getPropertyValue('--overlay-wave-mid')"
    )
    assert not errors
    context.close()


@pytest.mark.parametrize("locale", ["ru", "en"])
def test_cleanup_steps_through_the_pipeline(browser, locale):
    context = browser.new_context(viewport={"width": 1440, "height": 1000})
    page = context.new_page()
    page.clock.install()
    page.goto(site_url(locale))
    card = page.locator("[data-closeup]")
    text = card.locator(".closeup-text")
    final = text.inner_text()
    card.scroll_into_view_if_needed()
    page.clock.run_for(100)
    expect(card).to_have_class(re.compile(r"\bis-playing\b"))
    expect(card.locator('[data-closeup-step="0"]')).to_have_class(
        re.compile(r"\bis-current\b")
    )
    # The raw words differ from the cleaned sentence until the steps have run.
    assert (
        text.evaluate("el => [...el.children].map(p => p.textContent).join('')").split()
        != final.split()
    )
    last = card.locator('[data-closeup-step="5"]')
    run_until(page, lambda: "is-done" in (last.get_attribute("class") or ""))
    assert text.inner_text().split() == final.split()

    card.locator("[data-closeup-replay]").click()
    expect(card.locator('[data-closeup-step="0"]')).to_have_class(
        re.compile(r"\bis-current\b")
    )
    page.locator("#faq").scroll_into_view_if_needed()
    run_until(page, lambda: "is-playing" not in (card.get_attribute("class") or ""))
    assert text.inner_text().split() == final.split()
    context.close()


def test_features_walk_until_the_reader_picks_one(browser):
    context = browser.new_context(viewport={"width": 1440, "height": 1000})
    page = context.new_page()
    page.clock.install()
    page.goto(site_url("en"))
    caps = page.locator("[data-caps]")
    caps.scroll_into_view_if_needed()
    page.clock.run_for(100)
    expect(page.locator("#cap-tab-voice")).to_have_attribute("aria-selected", "true")
    # The level is a history: the readings move left as new ones arrive.
    bars = caps.locator('[data-cap="voice"] [data-ov-bar]')
    heights = "els => els.map((el) => el.style.height).join()"
    before = bars.evaluate_all(heights)
    page.clock.run_for(400)
    assert bars.evaluate_all(heights) != before
    page.clock.run_for(5000)
    expect(page.locator("#cap-tab-cleanup")).to_have_attribute("aria-selected", "true")

    page.locator("#cap-tab-history").click()
    page.clock.run_for(12000)
    expect(page.locator("#cap-tab-history")).to_have_attribute("aria-selected", "true")
    context.close()


def test_theme_follows_the_system_until_the_reader_picks_one(browser):
    light, dark = "rgb(242, 243, 240)", "rgb(17, 19, 23)"
    context = browser.new_context(
        viewport={"width": 1440, "height": 900}, color_scheme="light"
    )
    page = context.new_page()
    page.goto(site_url("en"))
    background = lambda: page.evaluate(
        "getComputedStyle(document.body).backgroundColor"
    )
    stored = lambda: page.evaluate("localStorage.getItem('sotto-theme')")
    toggle = page.locator("[data-theme-toggle]")
    assert background() == light
    expect(toggle).to_have_attribute("aria-pressed", "false")

    # The switch turns the other theme on and remembers it across a reload.
    toggle.click()
    expect(toggle).to_have_attribute("aria-pressed", "true")
    assert background() == dark
    assert stored() == "dark"
    page.reload()
    assert page.evaluate("document.documentElement.dataset.theme") == "dark"
    assert background() == dark

    # Picking what the system shows anyway forgets the choice, so the page follows the system again.
    toggle.click()
    assert background() == light
    assert stored() is None
    page.emulate_media(color_scheme="dark")
    assert background() == dark
    expect(toggle).to_have_attribute("aria-pressed", "true")
    context.close()


def test_apps_band_drifts_only_on_screen(browser):
    context = browser.new_context(viewport={"width": 1440, "height": 1000})
    page = context.new_page()
    page.goto(site_url("en"))
    band = page.locator("[data-apps]")
    expect(band).not_to_have_class(re.compile(r"\bis-playing\b"))
    band.scroll_into_view_if_needed()
    expect(band).to_have_class(re.compile(r"\bis-playing\b"))
    # The sprite resolves, so every tile shows a mark.
    assert page.evaluate(
        "async () => (await fetch('/app-logos.svg')).headers.get('content-type').startsWith('image/svg+xml')"
    )
    page.locator("#faq").scroll_into_view_if_needed()
    expect(band).not_to_have_class(re.compile(r"\bis-playing\b"))
    context.close()


@pytest.mark.parametrize("locale", ["ru", "en"])
def test_without_javascript(browser, locale):
    context = browser.new_context(java_script_enabled=False)
    page = context.new_page()
    page.goto(site_url(locale))
    desk = page.locator("[data-desk]")
    expect(desk).to_have_attribute("data-phase", "done")
    expect(desk.locator("[data-desk-text]")).not_to_have_text("")
    expect(desk.locator(".ov-done")).to_be_visible()
    expect(page.locator(".closeup-text")).not_to_have_text("")
    expect(page.locator(".closeup-foot")).to_be_hidden()
    expect(page.locator(".caps-panel:visible")).to_have_count(len(FEATURES))
    expect(page.locator(".caps-copy:visible")).to_have_count(len(FEATURES))
    expect(page.locator(".caps-list")).to_be_hidden()
    page.locator(".model-catalog summary").click()
    expect(page.locator(".model-card:visible")).to_have_count(
        page.locator(".model-card").count()
    )
    context.close()
