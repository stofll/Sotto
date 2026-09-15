import pytest
from playwright.sync_api import expect


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
    ui = app()
    ui.queue(
        "validate_hotkey",
        {"error": "Invalid shortcut"} if failure else {"result": None},
    )
    if not failure:
        ui.queue("set_hotkey", {"result": None})
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
    button.click()
    expect(button).to_have_attribute("aria-pressed", "false")


def test_microphone_meter_responds_to_events(app, page):
    ui = app()
    page.get_by_role("button", name="Проверка микрофона", exact=True).click()
    ui.emit("microphone-test-level", {"level": 1})
    expect(page.get_by_role("meter")).not_to_have_attribute("aria-valuenow", "0")
    page.get_by_role("button", name="Проверка микрофона", exact=True).click()
    expect(page.get_by_role("meter")).to_have_attribute("aria-valuenow", "0")


def test_overlay_preferences_persist_and_center_keeps_offset(app, page):
    ui = app(config={"ui_accent": "#e68a3d"})
    settings = page.get_by_test_id("overlay-settings")
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
    assert ui.state()["config"]["overlay"] == {
        "form": "bead",
        "size": "l",
        "edge_offset": 128,
        "anchor": "center",
    }
    page.reload()
    settings.get_by_role("button", name="Сверху слева", exact=True).click()
    expect(offset).to_be_enabled()
    expect(offset).to_have_value("128")
    expect(settings.get_by_role("button", name="Бусина", exact=True)).to_have_attribute(
        "aria-pressed", "true"
    )


def test_overlay_save_failure_rolls_back_and_retries(app, page):
    ui = app(config={"ui_accent": "#e68a3d"})
    settings = page.get_by_test_id("overlay-settings")
    ui.queue("save_config", {"error": "Synthetic disk full"})
    bead = settings.get_by_role("button", name="Бусина", exact=True)
    bead.click()
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
    assert page.evaluate(
        "getComputedStyle(document.documentElement).getPropertyValue('--accent').trim()"
    ) == (configured or "#9b75ef")


def test_custom_palette_saves_on_release_and_retains_other_fields(app, page):
    ui = app(
        config={
            "ui_accent": "#e68a3d",
            "overlay": {"palette": "custom", "form": "bead", "size": "l"},
        }
    )
    settings = page.get_by_test_id("overlay-settings")
    hue = settings.get_by_role("slider", name="Тон", exact=True)
    hue.focus()
    hue.press("ArrowRight")
    page.wait_for_function(
        "window.__sottoTest.state.config.overlay.palette_hue === 269"
    )
    assert ui.state()["config"]["overlay"]["form"] == "bead"
    assert ui.state()["config"]["overlay"]["size"] == "l"
    page.reload()
    expect(hue).to_have_value("269")


def test_failed_accent_migration_retains_legacy_value_for_restart(app, page):
    page.add_init_script("localStorage.setItem('sotto.ui.accent', '#9b75ef')")
    ui = app(responses={"save_config": [{"error": "Synthetic migration failure"}]})
    expect(page.get_by_role("alert")).to_contain_text("Synthetic migration failure")
    assert page.evaluate("localStorage.getItem('sotto.ui.accent')") == "#9b75ef"
    assert "ui_accent" not in ui.state()["config"]
