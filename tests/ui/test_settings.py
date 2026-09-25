from pathlib import Path

import pytest
from playwright.sync_api import expect


def open_overlay_settings(page):
    disclosure = page.get_by_test_id("overlay-disclosure")
    if disclosure.get_attribute("open") is None:
        disclosure.locator("summary").click()
    settings = disclosure.get_by_test_id("overlay-settings")
    expect(settings).to_be_visible()
    return settings


@pytest.mark.parametrize(
    "label,field,initial",
    [
        ("Авто-вставка текста", "auto_paste", True),
        ("Приглушать звук", "duck_output_while_recording", False),
    ],
)
def test_preferences_persist(app, page, label, field, initial):
    ui = app()
    control = page.get_by_role("checkbox", name=label, exact=True)
    expect(control).to_be_checked(checked=initial)
    control.click()
    ui.saved(field, not initial)
    ui.nav("models")
    ui.nav("settings")
    expect(page.get_by_role("checkbox", name=label, exact=True)).to_be_checked(
        checked=not initial
    )


def test_paste_options_depend_on_auto_paste(app, page):
    ui = app()
    page.get_by_text("Дополнительно", exact=True).click()
    trailing = page.get_by_role("checkbox", name="Пробел в конце", exact=True)
    trailing.check()
    ui.saved("paste_trailing_space", True)
    page.get_by_role("checkbox", name="Авто-вставка текста", exact=True).uncheck()
    expect(trailing).to_be_disabled()
    expect(trailing).to_be_checked()
    page.get_by_role("checkbox", name="Авто-вставка текста", exact=True).check()
    expect(trailing).to_be_enabled()


def test_setting_failure_and_retry(app, page):
    ui = app()
    ui.queue("save_config", {"error": "Synthetic disk full"})
    control = page.get_by_role("checkbox", name="Авто-вставка текста", exact=True)
    control.click()
    expect(page.get_by_role("alert")).to_contain_text("Synthetic disk full")
    expect(control).to_be_checked()
    control.click()
    expect(control).not_to_be_checked()
    ui.saved("auto_paste", False)


def test_recording_mode(app, page):
    ui = app()
    page.get_by_role("button", name="Удерживать", exact=True).click()
    ui.saved("recording_mode", "push_to_talk")
    page.reload()
    expect(page.get_by_role("button", name="Удерживать", exact=True)).to_have_attribute(
        "aria-pressed", "true"
    )


def test_locale_changes_live_and_persists(app, page):
    ui = app()
    page.get_by_role("button", name="English", exact=True).click()
    ui.saved("ui_language", "en")
    expect(page.get_by_role("heading", name="Settings", exact=True)).to_be_visible()
    page.reload()
    expect(page.get_by_role("heading", name="Settings", exact=True)).to_be_visible()


@pytest.mark.parametrize(
    "locale,next_locale,label", [("ru", "en", "English"), ("en", "ru", "Русский")]
)
@pytest.mark.parametrize("theme", ["light", "dark"])
def test_locale_save_failure_keeps_language_and_selection(
    app, page, locale, next_locale, label, theme, output_path
):
    ui = app(config={"ui_language": locale, "theme": theme})
    ui.queue("save_config", {"hold": True})
    button = page.get_by_role("button", name=label, exact=True)
    button.focus()
    expect(button).to_be_focused()
    Path(output_path).mkdir(parents=True, exist_ok=True)
    page.screenshot(
        path=str(Path(output_path) / "locale-focused.png"), animations="disabled"
    )
    button.press("Enter")
    expect(button).to_be_disabled()
    page.screenshot(
        path=str(Path(output_path) / "locale-pending.png"), animations="disabled"
    )
    expect(page.locator("html")).to_have_attribute("lang", locale)
    expect(button).to_have_attribute("aria-pressed", "false")
    ui.settle("save_config", error="Synthetic language save failure")
    expect(page.get_by_role("alert")).to_contain_text("Synthetic language save failure")
    expect(button).to_be_enabled()
    expect(page.locator("html")).to_have_attribute("lang", locale)
    expect(button).to_have_attribute("aria-pressed", "false")
    assert ui.state()["config"]["ui_language"] == locale
    button.click()
    ui.saved("ui_language", next_locale)
    expect(page.locator("html")).to_have_attribute("lang", next_locale)
    expect(button).to_have_attribute("aria-pressed", "true")


def test_microphone_selection(app, page):
    ui = app()
    page.get_by_role(
        "button", name="Системный микрофон по умолчанию", exact=True
    ).click()
    page.get_by_role("option", name="Synthetic microphone").click()
    ui.saved("microphone", "test-mic")
    expect(
        page.get_by_role("button", name="Synthetic microphone", exact=True)
    ).to_be_visible()


@pytest.mark.parametrize("failure", [False, True])
def test_hotkey_validation(app, page, failure):
    ui = app(config={"ui_accent": "#e68a3d"})
    ui.queue(
        "validate_hotkey",
        {"error": "Invalid shortcut"} if failure else {"result": None},
    )
    page.get_by_role("button", name="Изменить", exact=True).click()
    field = page.get_by_test_id("hotkey-input")
    field.fill("ctrl+alt+k")
    field.press("Enter")
    if failure:
        expect(page.get_by_text("Invalid shortcut", exact=False)).to_be_visible()
        assert ui.state()["config"]["hotkey"] == "Ctrl+Shift+Space"
    else:
        ui.saved("hotkey", "ctrl+alt+k")
        expect(field).not_to_be_visible()
        assert len(ui.calls("save_config")) == 1


@pytest.mark.parametrize("capture", [False, True])
def test_hotkey_failed_save_keeps_draft_and_retries_one_transaction(app, page, capture):
    ui = app(config={"ui_accent": "#e68a3d"})
    page.get_by_role("button", name="Изменить", exact=True).click()
    field = page.get_by_test_id("hotkey-input")
    ui.queue("validate_hotkey", {"result": None}, {"result": None})
    ui.queue("save_config", {"hold": True})
    if capture:
        page.get_by_role("button", name="Записать", exact=True).click()
        page.keyboard.press("Control+Alt+k")
    else:
        field.fill("ctrl+alt+k")
        field.press("Enter")
    apply = page.get_by_role("button", name="Применить", exact=True)
    expect(field).to_be_disabled()
    expect(apply).to_be_disabled()
    expect(field).to_have_value("ctrl+alt+k")
    # Even a synthetic second key event cannot enqueue another transaction.
    field.dispatch_event("keydown", {"key": "Enter"})
    ui.settle("save_config", error="Synthetic hotkey persistence failure")
    expect(page.get_by_role("alert")).to_contain_text(
        "Synthetic hotkey persistence failure"
    )
    expect(field).to_be_enabled()
    expect(field).to_have_value("ctrl+alt+k")
    assert len(ui.calls("save_config")) == 1
    assert ui.state()["config"]["hotkey"] == "Ctrl+Shift+Space"
    apply.click()
    ui.saved("hotkey", "ctrl+alt+k")
    expect(field).not_to_be_visible()
    assert len(ui.calls("save_config")) == 2


def test_hotkey_escape_leaves_config_unchanged(app, page):
    ui = app(config={"ui_accent": "#e68a3d"})
    page.get_by_role("button", name="Изменить", exact=True).click()
    field = page.get_by_test_id("hotkey-input")
    expect(field).to_be_focused()
    field.fill("ctrl+alt+k")
    field.press("Escape")
    expect(field).not_to_be_visible()
    assert not ui.calls("save_config")


def test_sound_volume_and_disable(app, page):
    ui = app()
    page.get_by_role("button", name="Средне", exact=True).click()
    page.get_by_role("option", name="Выключено", exact=True).click()
    ui.saved("sound_feedback", False)
    page.get_by_role("button", name="Выключено", exact=True).click()
    page.get_by_role("option", name="Средне", exact=True).click()
    ui.saved("sound_feedback", True)
    assert ui.state()["config"]["sound_volume"] == 0.35


@pytest.mark.parametrize(
    "label,field",
    [
        ("Запускать вместе с системой", "auto_start"),
        ("Разрешить обезличенную телеметрию", "telemetry_enabled"),
        ("Enter после вставки", "paste_auto_submit"),
    ],
)
def test_advanced_preferences(app, page, label, field):
    ui = app()
    page.get_by_text("Дополнительно", exact=True).click()
    page.get_by_role("checkbox", name=label, exact=True).check()
    ui.saved(field, True)
    page.reload()
    page.get_by_text("Дополнительно", exact=True).click()
    expect(page.get_by_role("checkbox", name=label, exact=True)).to_be_checked()


def test_portable_mode_disables_autostart(app, page):
    app(runtime={"portable": True})
    page.get_by_text("Дополнительно", exact=True).click()
    expect(
        page.get_by_role("checkbox", name="Запускать вместе с системой", exact=True)
    ).to_be_disabled()


def test_microphone_test_error_retry_and_stop(app, page):
    ui = app()
    ui.queue("start_microphone_test", {"error": "Synthetic device unavailable"})
    button = page.get_by_role("button", name="Проверка микрофона", exact=True)
    button.click()
    expect(page.get_by_role("alert")).to_contain_text("Synthetic device unavailable")
    button.click()
    expect(button).to_have_attribute("aria-pressed", "true")
    expect(page.locator(".mic-control [role=status]")).to_have_count(0)
    button.click()
    expect(button).to_have_attribute("aria-pressed", "false")
    expect(page.locator(".mic-control [role=status]")).to_have_count(0)
    button.click()
    ui.emit("microphone-test-stopped", None)
    expect(button).to_have_attribute("aria-pressed", "false")
    expect(page.locator(".mic-control [role=status]")).to_have_count(0)


def test_microphone_meter_responds_to_events(app, page):
    ui = app()
    page.get_by_role("button", name="Проверка микрофона", exact=True).click()
    ui.emit("microphone-test-level", {"level": 1})
    expect(page.get_by_role("meter")).not_to_have_attribute("aria-valuenow", "0")
    page.get_by_role("button", name="Проверка микрофона", exact=True).click()
    expect(page.get_by_role("meter")).to_have_attribute("aria-valuenow", "0")


def test_overlay_preferences_persist_and_center_keeps_offset(app, page):
    ui = app(config={"ui_accent": "#e68a3d"})
    settings = open_overlay_settings(page)
    settings.get_by_role("button", name="Бусина", exact=True).click()
    expect(settings.get_by_test_id("bead-hint")).to_contain_text(
        "потоковый текст не отображается"
    )
    settings.get_by_role("button", name="L", exact=True).click()
    offset = settings.get_by_label("Отступ от края", exact=True)
    offset.fill("128")
    offset.press("Tab")
    settings.get_by_role("button", name="По центру", exact=True).click()
    expect(offset).to_be_disabled()
    steppers = settings.locator(".number-field__steppers button")
    for button in steppers.all():
        expect(button).to_be_disabled()
        button.evaluate("e => e.click()")
    assert ui.state()["config"]["overlay"] == {
        "form": "bead",
        "size": "l",
        "edge_offset": 128,
        "anchor": "center",
    }
    page.reload()
    open_overlay_settings(page)
    settings.get_by_role("button", name="Сверху слева", exact=True).click()
    expect(offset).to_be_enabled()
    expect(offset).to_have_value("128")
    steppers.first.click()
    expect(offset).to_have_value("129")
    page.wait_for_function(
        "window.__sottoTest.state.config.overlay.edge_offset === 129"
    )
    expect(settings.get_by_role("button", name="Бусина", exact=True)).to_have_attribute(
        "aria-pressed", "true"
    )


def test_overlay_save_failure_rolls_back_and_retries(app, page):
    ui = app(config={"ui_accent": "#e68a3d"})
    settings = open_overlay_settings(page)
    ui.queue("save_config", {"hold": True})
    bead = settings.get_by_role("button", name="Бусина", exact=True)
    bead.click()
    for button in settings.locator(".number-field__steppers button").all():
        expect(button).to_be_disabled()
        button.evaluate("e => e.click()")
    assert len(ui.calls("save_config")) == 1
    ui.settle("save_config", error="Synthetic disk full")
    expect(settings.get_by_role("alert")).to_contain_text("Не удалось сохранить")
    expect(settings.get_by_role("button", name="Пилюля", exact=True)).to_have_attribute(
        "aria-pressed", "true"
    )
    bead.click()
    expect(bead).to_have_attribute("aria-pressed", "true")
    expect(settings.get_by_role("alert")).to_have_count(0)


@pytest.mark.parametrize("configured", [None, "#5b8def"])
def test_accent_migration_respects_existing_config(app, page, configured):
    page.add_init_script("localStorage.setItem('sotto.ui.accent', '#9b75ef')")
    ui = app(config={"ui_accent": configured} if configured else {})
    ui.saved("ui_accent", configured or "#9b75ef")
    expect(page.locator("html")).to_have_css("--accent", configured or "#9b75ef")


def test_overlay_palette_swatches_save_and_name_on_hover(app, page):
    ui = app()
    settings = open_overlay_settings(page)
    palette = settings.get_by_role("group", name="Цвет оверлея", exact=True)
    expect(palette.get_by_role("button", name="Графит", exact=True)).to_have_attribute(
        "aria-pressed", "true"
    )
    lagoon = palette.get_by_role("button", name="Лагуна", exact=True)
    lagoon.click()
    ui.saved("overlay", {"palette": "lagoon"})
    expect(lagoon).to_have_attribute("aria-pressed", "true")
    expect(lagoon).to_have_css("border-radius", "4px")
    label = palette.locator(".set-label")
    label_box, swatch_box = label.bounding_box(), lagoon.bounding_box()
    assert label_box["y"] + label_box["height"] <= swatch_box["y"] + 1
    # The name is no longer printed under the row; hovering says it instead.
    lagoon.hover()
    expect(page.get_by_role("tooltip", name="Лагуна", exact=True)).to_be_visible()
    # The interface colour is not an overlay setting any more.
    expect(settings.get_by_text("Акцент приложения")).to_have_count(0)


@pytest.mark.parametrize(
    "locale,group_label,graphite_label",
    [
        ("ru", "Цвет оверлея", "Графит"),
        ("en", "Overlay colour", "Graphite"),
    ],
)
@pytest.mark.parametrize("theme", ["light", "dark"])
def test_overlay_defaults_to_graphite_in_both_themes(
    app, page, locale, group_label, graphite_label, theme, output_path
):
    app(config={"ui_language": locale, "theme": theme})
    settings = open_overlay_settings(page)
    palette = settings.get_by_role("group", name=group_label, exact=True)
    graphite = palette.get_by_role("button", name=graphite_label, exact=True)
    expect(graphite).to_have_attribute("aria-pressed", "true")
    graphite.focus()
    expect(graphite).to_be_focused()
    assert " 0 0 " in settings.get_by_test_id("overlay-preview").get_attribute("style")
    Path(output_path).mkdir(parents=True, exist_ok=True)
    page.screenshot(
        path=str(Path(output_path) / "overlay-default.png"), animations="disabled"
    )


def test_overlay_timer_toggle_saves(app, page):
    ui = app()
    settings = open_overlay_settings(page)
    control = settings.get_by_role("checkbox", name="Секундомер", exact=True)
    expect(control).to_be_checked()
    control.click()
    ui.saved("overlay", {"show_timer": False})
    expect(control).not_to_be_checked()


def test_interface_color_presets_and_custom_value(app, page):
    ui = app(config={"ui_accent": "#e68a3d"})
    page.get_by_text("Дополнительно", exact=True).click()
    cell = page.locator(".advanced__appearance-cell")
    picker = page.get_by_role("group", name="Цвет интерфейса", exact=True)
    label = cell.locator(".set-label")
    assert label.bounding_box()["x"] < picker.bounding_box()["x"]
    picker.get_by_role("button", name="Зелёный", exact=True).click()
    ui.saved("ui_accent", "#3dc97c")
    assert (
        page.evaluate(
            "getComputedStyle(document.documentElement).getPropertyValue('--accent').trim()"
        )
        == "#3dc97c"
    )
    # Any colour goes, not only the four presets.
    picker.get_by_label("Свой цвет", exact=True).fill("#123456")
    ui.saved("ui_accent", "#123456")


@pytest.mark.parametrize("custom", [False, True])
def test_interface_color_save_failure_rolls_back_and_retries(app, page, custom):
    ui = app(config={"ui_accent": "#e68a3d"})
    page.get_by_text("Дополнительно", exact=True).click()
    picker = page.get_by_role("group", name="Цвет интерфейса", exact=True)
    ui.queue("save_config", {"error": "Synthetic accent save failure"})

    def pick():
        if custom:
            picker.get_by_label("Свой цвет", exact=True).fill("#123456")
        else:
            picker.get_by_role("button", name="Зелёный", exact=True).click()

    pick()
    expect(page.get_by_role("alert")).to_contain_text("Synthetic accent save failure")
    expect(
        picker.get_by_role("button", name="Оранжевый", exact=True)
    ).to_have_attribute("aria-pressed", "true")
    assert ui.state()["config"]["ui_accent"] == "#e68a3d"
    expect(page.locator("html")).to_have_css("--accent", "#e68a3d")
    pick()
    ui.saved("ui_accent", "#123456" if custom else "#3dc97c")
    page.reload()
    expect(page.locator("html")).to_have_css(
        "--accent", "#123456" if custom else "#3dc97c"
    )


def test_leaving_settings_cancels_unsaved_color_preview(app, page):
    page.clock.install(time="2026-01-01T00:00:00Z")
    ui = app(config={"ui_accent": "#e68a3d"})
    page.get_by_text("Дополнительно", exact=True).click()
    # Installing the clock still lets time pass during browser actions.
    # Pause before picking so navigation cannot race the 250 ms save timer.
    page.clock.pause_at("2026-01-01T00:01:00Z")
    page.get_by_role("group", name="Цвет интерфейса", exact=True).get_by_label(
        "Свой цвет", exact=True
    ).fill("#123456")
    expect(page.locator("html")).to_have_css("--accent", "#123456")
    ui.nav("models")
    page.clock.run_for(300)
    assert not ui.calls("save_config")
    assert ui.state()["config"]["ui_accent"] == "#e68a3d"
    expect(page.locator("html")).to_have_css("--accent", "#e68a3d")


@pytest.mark.parametrize("saved_offset", [20, 200])
def test_offset_reset_returns_to_default(app, page, saved_offset):
    ui = app(config={"overlay": {"edge_offset": saved_offset}})
    settings = open_overlay_settings(page)
    reset = settings.get_by_role("button", name="Сбросить отступ", exact=True)
    reset.click()
    ui.saved("overlay", {"edge_offset": 25})
    expect(settings.get_by_label("Отступ от края", exact=True)).to_have_value("25")
    expect(reset).to_be_disabled()


@pytest.mark.parametrize(
    "form,size,shell",
    [
        ("pill", "s", ("280px", "52px")),
        ("pill", "m", ("308px", "56px")),
        ("pill", "l", ("360px", "64px")),
        ("bead", "s", ("56px", "56px")),
        ("bead", "m", ("64px", "64px")),
        ("bead", "l", ("72px", "72px")),
        ("glow", "s", ("360px", "92px")),
        ("glow", "m", ("400px", "104px")),
        ("glow", "l", ("440px", "116px")),
    ],
)
def test_overlay_preview_matches_real_geometry(app, page, form, size, shell):
    app(config={"overlay": {"form": form, "size": size}})
    open_overlay_settings(page)
    screen = page.get_by_test_id("overlay-preview")
    # The layout size is the real logical size; only a transform shrinks it to
    # fit the screen rectangle, so CSS is asked rather than the bounding box.
    mini = screen.locator(".overlay-mini__shell")
    expect(mini).to_have_css("width", shell[0])
    expect(mini).to_have_css("height", shell[1])


def test_overlay_position_zones_are_marked_before_hovering(app, page):
    app()
    open_overlay_settings(page)
    page.mouse.move(0, 0)
    screen = page.get_by_test_id("overlay-preview")
    # Every place the overlay can take is visible on its own; only the chosen
    # one drops its mark, because the preview of the overlay stands there.
    marks = screen.evaluate(
        "el => [...el.querySelectorAll('.overlay-screen__zone')]"
        ".map(zone => Number(getComputedStyle(zone, '::after').opacity))"
    )
    assert len(marks) == 9
    assert sum(1 for value in marks if value > 0) == 8
    chosen = screen.get_by_role("button", name="Снизу по центру", exact=True)
    assert chosen.evaluate("el => getComputedStyle(el, '::after').opacity") == "0"


def test_overlay_position_is_chosen_on_the_screen_preview(app, page):
    ui = app()
    open_overlay_settings(page)
    screen = page.get_by_test_id("overlay-preview")
    zone = screen.get_by_role("button", name="Сверху справа", exact=True)
    mini = screen.locator(".overlay-mini")
    zone.click()
    ui.saved("overlay", {"anchor": "top-right"})
    expect(zone).to_have_attribute("aria-pressed", "true")
    expect(mini).to_have_attribute("data-anchor", "top-right")
    # The offset moves the overlay away from the edges it is pinned to.
    near = mini.bounding_box()
    settings = open_overlay_settings(page)
    offset = settings.get_by_label("Отступ от края", exact=True)
    offset.fill("400")
    offset.press("Tab")
    far = mini.bounding_box()
    assert far["y"] > near["y"] and far["x"] + far["width"] < near["x"] + near["width"]


@pytest.mark.parametrize("form", ["pill", "bead"])
@pytest.mark.parametrize(
    "anchor",
    [
        "top-left",
        "top-center",
        "top-right",
        "center-left",
        "center",
        "center-right",
        "bottom-left",
        "bottom-center",
        "bottom-right",
    ],
)
def test_preview_offset_uses_window_scale_throughout_range(app, page, form, anchor):
    app(config={"overlay": {"form": form, "anchor": anchor, "edge_offset": 0}})
    open_overlay_settings(page)
    screen = page.get_by_test_id("overlay-preview")
    mini = screen.locator(".overlay-mini")
    field = page.get_by_label("Отступ от края", exact=True)
    window_width, window_height = (72, 72) if form == "bead" else (308, 64)
    # Resizing must preserve the logical distances, including values above the
    # old 250px saturation point, for every anchored edge.
    for viewport in [1280, 1000]:
        page.set_viewport_size({"width": viewport, "height": 1000})
        # The responsive sidebar animates the available panel width.
        page.locator(".win__layout").evaluate(
            "e => Promise.all(e.getAnimations().map(animation => animation.finished))"
        )
        for offset in [0] if anchor == "center" else [0, 20, 250, 400, 512]:
            if anchor != "center":
                field.fill(str(offset))
                field.press("Tab")
                expect(field).to_be_enabled()
            screen.scroll_into_view_if_needed()
            expect(mini).to_have_js_property("clientWidth", window_width)
            bounds, box = screen.evaluate(
                "e => [e.getBoundingClientRect().toJSON(), "
                "e.querySelector('.overlay-mini').getBoundingClientRect().toJSON()]"
            )
            scale = bounds["width"] / 1920
            assert box["width"] == pytest.approx(window_width * scale, abs=0.1)
            assert box["height"] == pytest.approx(window_height * scale, abs=0.1)
            x = (box["x"] - bounds["x"]) / scale
            y = (box["y"] - bounds["y"]) / scale
            expected_x = (1920 - window_width) / 2
            expected_y = (1080 - window_height) / 2
            if anchor.endswith("left"):
                expected_x = offset
            elif anchor.endswith("right"):
                expected_x = 1920 - window_width - offset
            if anchor.startswith("top"):
                expected_y = offset
            elif anchor.startswith("bottom"):
                expected_y = 1080 - window_height - offset
            assert x == pytest.approx(expected_x, abs=1)
            assert y == pytest.approx(expected_y, abs=1)
            assert 0 <= x + 1 and x + window_width <= 1921
            assert 0 <= y + 1 and y + window_height <= 1081


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["dark", "light"])
@pytest.mark.parametrize("form", ["pill", "bead"])
def test_position_preview_visuals(app, page, locale, theme, form, output_path):
    app(
        config={
            "ui_language": locale,
            "theme": theme,
            "overlay": {
                "form": form,
                "size": "s",
                "anchor": "center-right",
                "edge_offset": 25,
            },
        }
    )
    expect(page.get_by_test_id("overlay-settings")).not_to_be_visible()
    summary = page.get_by_test_id("overlay-disclosure").locator("summary")
    summary.focus()
    summary.press("Enter")
    settings = open_overlay_settings(page)
    screen = page.get_by_test_id("overlay-preview")
    selected = screen.locator('[aria-pressed="true"]')
    selected.focus()
    selected.press("Space")
    expect(selected).to_be_focused()
    expect(selected).to_have_css("outline-style", "solid")
    shots = Path(output_path)
    shots.mkdir(parents=True, exist_ok=True)
    settings.screenshot(path=str(shots / "offset-25.png"), animations="disabled")
    field = settings.locator("#overlay-offset")
    field.fill("400")
    field.press("Tab")
    settings.locator(".set-label").first.click()
    page.mouse.move(0, 0)
    settings.screenshot(path=str(shots / "offset-400.png"), animations="disabled")


def test_custom_palette_saves_on_release_and_retains_other_fields(app, page):
    ui = app(
        config={
            "ui_accent": "#e68a3d",
            "overlay": {"palette": "custom", "form": "bead", "size": "l"},
        }
    )
    settings = open_overlay_settings(page)
    hue = settings.get_by_role("slider", name="Тон", exact=True)
    hue.focus()
    hue.press("ArrowRight")
    page.wait_for_function(
        "window.__sottoTest.state.config.overlay.palette_hue === 269"
    )
    assert ui.state()["config"]["overlay"]["form"] == "bead"
    assert ui.state()["config"]["overlay"]["size"] == "l"
    page.reload()
    open_overlay_settings(page)
    expect(hue).to_have_value("269")


def test_failed_accent_migration_retains_legacy_value_for_restart(app, page):
    page.add_init_script("localStorage.setItem('sotto.ui.accent', '#9b75ef')")
    ui = app(responses={"save_config": [{"error": "Synthetic migration failure"}]})
    expect(page.get_by_role("alert")).to_contain_text("Synthetic migration failure")
    assert page.evaluate("localStorage.getItem('sotto.ui.accent')") == "#9b75ef"
    assert "ui_accent" not in ui.state()["config"]


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["dark", "light"])
def test_overlay_and_advanced_are_independent_collapsed_sections(
    app, page, locale, theme, output_path
):
    app(config={"ui_language": locale, "theme": theme})
    overlay = page.get_by_test_id("overlay-disclosure")
    advanced = page.get_by_test_id("advanced-settings")
    expect(overlay).not_to_have_attribute("open", "")
    expect(advanced).not_to_have_attribute("open", "")
    assert overlay.evaluate(
        "e => e.parentElement === document.querySelector('[data-testid=advanced-settings]').parentElement"
    )
    summary = overlay.locator("summary")
    summary.focus()
    summary.press("Enter")
    expect(page.get_by_test_id("overlay-settings")).to_be_visible()
    expect(advanced).not_to_have_attribute("open", "")
    timer = overlay.get_by_role(
        "checkbox", name="Секундомер" if locale == "ru" else "Timer", exact=True
    )
    label = timer.locator("..")
    swatches = overlay.locator(".overlay-swatches").bounding_box()
    timer_box = label.bounding_box()
    assert timer_box["x"] + timer_box["width"] <= swatches["x"]
    assert (
        abs(
            timer_box["y"]
            + timer_box["height"] / 2
            - swatches["y"]
            - swatches["height"] / 2
        )
        < 1
    )
    caption = overlay.locator(".set-label").first
    for prop in ["font-size", "font-weight", "font-family", "color"]:
        expect(label).to_have_css(
            prop,
            caption.evaluate("(e, p) => getComputedStyle(e).getPropertyValue(p)", prop),
        )
    advanced.locator("summary").click()
    expect(overlay).to_have_attribute("open", "")
    expect(advanced).to_have_attribute("open", "")
    shots = Path(output_path)
    shots.mkdir(parents=True, exist_ok=True)
    overlay.screenshot(path=str(shots / "overlay-section.png"), animations="disabled")
    summary.click()
    expect(page.get_by_test_id("overlay-settings")).not_to_be_visible()
    expect(advanced).to_have_attribute("open", "")
