"""Observable idle-work regressions, not native CPU or frame-time benchmarks."""

import re

import pytest
from playwright.sync_api import expect

BEAM_RECIPE = {
    "shell": "card",
    "slots": {"edge": "level", "footL": "timer"},
    "draw": {"level": "beam"},
    "processing": {"draw": "matrix"},
}


def observe_preview_work(page):
    page.add_init_script("""(() => {
      const work = window.__previewWork = { frames: 0, intervals: 0, beamWrites: 0 };
      const pendingFrames = window.__previewPendingFrames = new Set();
      window.__previewWorkAt = performance.now();
      const record = key => { work[key]++; window.__previewWorkAt = performance.now(); };
      const frame = window.requestAnimationFrame;
      window.requestAnimationFrame = callback => {
        const id = frame.call(window, now => {
          pendingFrames.delete(id);
          record('frames');
          callback(now);
        });
        pendingFrames.add(id);
        return id;
      };
      const cancelFrame = window.cancelAnimationFrame;
      window.cancelAnimationFrame = id => {
        pendingFrames.delete(id);
        return cancelFrame.call(window, id);
      };
      const interval = window.setInterval;
      window.setInterval = (callback, delay, ...args) => interval.call(window,
        typeof callback === 'function' ? (...values) => {
          record('intervals');
          callback(...values);
        } : callback, delay, ...args);
      const setProperty = CSSStyleDeclaration.prototype.setProperty;
      CSSStyleDeclaration.prototype.setProperty = function (name, ...args) {
        if (name.startsWith('--vb-')) record('beamWrites');
        return setProperty.call(this, name, ...args);
      };
    })();""")


def preview_work(page):
    before = page.evaluate("({...window.__previewWork})")
    # Count recurring work after settling; no assertion depends on machine speed.
    page.wait_for_timeout(250)
    after = page.evaluate("({...window.__previewWork})")
    return {key: after[key] - before[key] for key in before}


def expect_animated_preview(page):
    # Require new frames and paint writes, not cumulative work from mounting or
    # a short fixed sample that can land before a throttled browser resumes.
    before = page.evaluate("({...window.__previewWork})")
    page.wait_for_function(
        "before => window.__previewWork.frames >= before.frames + 3 && "
        "window.__previewWork.beamWrites >= before.beamWrites + 100",
        arg=before,
        polling=50,
        timeout=10000,
    )


def expect_idle_preview(page):
    # A throttled browser can deliver the final 600 ms settling frame late.
    # Quiet counters alone can miss a queued frame: require its delivery or
    # cancellation, then a quiet period and exactly zero further work.
    page.wait_for_function(
        "() => window.__previewPendingFrames.size === 0 && "
        "performance.now() - window.__previewWorkAt >= 750",
        polling=50,
        timeout=10000,
    )
    assert preview_work(page) == {"frames": 0, "intervals": 0, "beamWrites": 0}


def open_editor(page):
    page.get_by_test_id("overlay-disclosure").locator("summary").click()
    page.get_by_role("button", name="Открыть конструктор", exact=True).click()
    editor = page.get_by_test_id("overlay-editor")
    expect(editor).to_be_visible()
    return editor


def test_collapsed_settings_do_not_load_glow_and_open_thumbnails_stay_idle(app, page):
    observe_preview_work(page)
    requests = []
    page.on("request", lambda request: requests.append(request.url))
    app()
    disclosure = page.get_by_test_id("overlay-disclosure")
    expect(disclosure).not_to_have_attribute("open", "")
    expect(page.locator("[data-voice-beam]")).to_have_count(0)
    expect_idle_preview(page)
    assert not any("/OverlayGlow" in url for url in requests)

    for _ in range(2):
        disclosure.locator("summary").click()
        beam = disclosure.locator("[data-voice-beam]")
        expect(beam).to_be_visible()
        page.wait_for_function("""() => {
          const canvas = document.querySelector('[data-voice-beam-band]');
          return canvas && canvas.width > 0 && canvas.height > 0;
        }""")
        expect_idle_preview(page)
        disclosure.locator("summary").click()
        expect(beam).to_have_count(0)
        expect_idle_preview(page)


@pytest.mark.parametrize("processing", [False, True])
def test_constructor_pauses_on_native_blur_and_resumes_without_losing_recipe(
    app, page, processing
):
    observe_preview_work(page)
    ui = app(config={"overlay": {"recipe": BEAM_RECIPE}})
    editor = open_editor(page)
    if processing:
        editor.locator(".ove-phases").get_by_role(
            "button", name="Обработка", exact=True
        ).click()
    expect(editor.locator(".ove-stage [data-voice-beam]")).to_be_visible()
    expect_animated_preview(page)
    recipe = ui.state()["config"]["overlay"]["recipe"]

    ui.emit("tauri://blur")
    assert page.evaluate("document.visibilityState") == "visible"
    expect_idle_preview(page)
    # Pausing the drawing must preserve the constructor's interactive targets.
    expect(editor.locator(".ove-stage button[data-cancel]")).to_have_count(1)
    assert ui.state()["config"]["overlay"]["recipe"] == recipe

    ui.emit("tauri://focus")
    expect(editor.locator(".ove-stage .ovs")).not_to_have_class(
        re.compile(r".*ovs--still.*")
    )
    expect_animated_preview(page)
    assert ui.state()["config"]["overlay"]["recipe"] == recipe


@pytest.mark.parametrize(
    "seed", [{"window_visible": False}, {"window_minimized": True}]
)
def test_constructor_stays_idle_when_native_window_is_hidden(app, page, seed):
    observe_preview_work(page)
    app(config={"overlay": {"recipe": BEAM_RECIPE}}, **seed)
    open_editor(page)
    assert page.evaluate("document.visibilityState") == "visible"
    expect_idle_preview(page)


@pytest.mark.parametrize("recipe", [False, True])
def test_unfocused_real_overlay_keeps_animating_and_stops_after_reset(
    app, page, recipe
):
    observe_preview_work(page)
    ui = app(
        "overlay",
        window_focused=False,
        config={"overlay": {"recipe": BEAM_RECIPE} if recipe else {"form": "glow"}},
    )
    ui.emit("recording-started", 42)
    expect(page.locator("[data-voice-beam]")).to_be_visible()
    ui.emit("audio-level", {"level": 0.7})
    expect_animated_preview(page)
    ui.emit("overlay-reset")
    expect(page.locator("[data-voice-beam]")).to_have_count(0)
    expect_idle_preview(page)
