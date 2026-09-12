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
    ui = app()
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
