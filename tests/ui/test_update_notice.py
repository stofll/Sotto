from pathlib import Path

import pytest
from playwright.sync_api import expect

UPDATE = {"available": True, "current_version": "0.3.0", "version": "0.3.1"}
CHECK_INTERVAL = 6 * 60 * 60 * 1000
DAY = 24 * 60 * 60 * 1000
RELEASE = {
    "version": "0.3.0",
    "notes": "Synthetic notes",
    "url": "https://github.com/stofll/Sotto/releases",
}


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["dark", "light"])
def test_notice_layout_focus_and_details(app, page, locale, theme, output_path):
    page.set_viewport_size({"width": 760, "height": 620})
    ui = app(
        config={"ui_language": locale, "theme": theme},
        responses={"check_update": [{"hold": True}]},
    )
    opener = page.get_by_test_id("nav-settings")
    opener.focus()
    ui.settle("check_update", result=UPDATE)
    notice = page.get_by_test_id("update-notice")
    expect(notice).to_contain_text("0.3.1")
    expect(opener).to_be_focused()
    box = notice.bounding_box()
    assert box["y"] < 100 and box["x"] >= 0
    assert box["x"] + box["width"] <= 760
    details = notice.get_by_role(
        "button", name="Подробнее" if locale == "ru" else "Details", exact=True
    )
    Path(output_path).mkdir(parents=True, exist_ok=True)
    notice.screenshot(
        path=str(Path(output_path) / f"notice-{locale}-{theme}.png"),
        animations="disabled",
    )
    page.screenshot(
        path=str(Path(output_path) / f"window-{locale}-{theme}.png"),
        animations="disabled",
    )
    details.focus()
    expect(details).to_be_focused()
    notice.screenshot(
        path=str(Path(output_path) / f"notice-focused-{locale}-{theme}.png"),
        animations="disabled",
    )
    ui.queue("check_update", {"result": UPDATE})
    page.keyboard.press("Enter")
    expect(notice).not_to_be_visible()
    expect(page.get_by_test_id("page-info")).to_be_visible()
    expect(
        page.get_by_role(
            "button",
            name="Обновить до 0.3.1" if locale == "ru" else "Update to 0.3.1",
            exact=True,
        )
    ).to_be_visible()
    assert not ui.calls("install_update")


def test_notice_does_not_consume_a_reminder_while_window_is_hidden(app, page):
    page.clock.install()
    page.add_init_script(
        "Object.defineProperty(document, 'visibilityState', {configurable: true, value: 'hidden'})"
    )
    ui = app(responses={"check_update": [{"hold": True}]})
    assert not ui.calls("check_update")
    page.evaluate("""() => {
        Object.defineProperty(document, 'visibilityState', {configurable: true, value: 'visible'});
        document.dispatchEvent(new Event('visibilitychange'));
    }""")
    ui.settle("check_update", result=UPDATE)
    expect(page.get_by_test_id("update-notice")).to_be_visible()


def test_notice_timer_recovers_after_dictation_interrupts_hover(app, page):
    page.clock.install()
    ui = app(responses={"check_update": [{"result": UPDATE}]})
    notice = page.get_by_test_id("update-notice")
    expect(notice).to_be_visible()
    notice.hover()
    ui.emit("recording-started", 1)
    expect(notice).not_to_be_visible()
    page.mouse.move(0, 500)
    ui.emit("whisper-empty", 1)
    expect(notice).to_be_visible()
    page.clock.fast_forward(13_000)
    expect(notice).not_to_be_visible()


def test_notice_auto_dismiss_pauses_for_keyboard_and_survives_restart(app, page):
    page.clock.install()
    ui = app(responses={"check_update": [{"result": UPDATE}]})
    notice = page.get_by_test_id("update-notice")
    expect(notice).to_be_visible()
    notice.get_by_role("button", name="Подробнее", exact=True).focus()
    notice.hover()
    page.mouse.move(0, 500)
    page.clock.run_for(20_000)
    expect(notice).to_be_visible()
    page.get_by_test_id("nav-settings").focus()
    page.clock.run_for(12_001)
    expect(notice).not_to_be_visible()
    assert ui.state()["config"]["update_reminder_shown_at"] > 0
    ui.queue("check_update", {"result": UPDATE})
    page.reload()
    expect(page.get_by_test_id("page-settings")).to_be_visible()
    expect(notice).not_to_be_visible()


def test_failed_reminder_save_stays_quiet_and_keeps_launch_limit(app, page):
    page.clock.install()
    ui = app(
        config={"ui_accent": "#e68a3d"},
        responses={
            "check_update": [{"result": UPDATE}],
            "save_config": [{"error": "Synthetic reminder save failure"}],
        },
    )
    notice = page.get_by_test_id("update-notice")
    expect(notice).to_be_visible()
    expect(page.get_by_role("alert")).not_to_be_visible()
    assert "update_reminder_shown_at" in ui.calls("save_config")[-1]["args"]["patch"]
    assert "update_reminder_shown_at" not in ui.state()["config"]
    notice.get_by_role("button", name="Закрыть уведомление", exact=True).click()
    ui.queue("check_update", {"result": UPDATE})
    page.clock.fast_forward(CHECK_INTERVAL + 1_000)
    expect(notice).not_to_be_visible()


def test_dismissed_notice_reappears_after_a_day(app, page):
    page.clock.install()
    ui = app(responses={"check_update": [{"result": UPDATE}]})
    notice = page.get_by_test_id("update-notice")
    expect(notice).to_be_visible()
    notice.get_by_role("button", name="Закрыть уведомление", exact=True).click()
    ui.queue("check_update", {"result": UPDATE})
    page.clock.fast_forward(DAY - CHECK_INTERVAL)
    expect(notice).not_to_be_visible()
    ui.queue("check_update", {"result": UPDATE})
    page.clock.fast_forward(CHECK_INTERVAL + 1_000)
    expect(notice).to_be_visible()
    assert not ui.calls("install_update")


@pytest.mark.parametrize(
    "finish", ["whisper-empty", "whisper-cancelled", "paste-done", "whisper-failed"]
)
def test_notice_waits_for_dictation(app, page, finish):
    page.clock.install()
    ui = app(responses={"check_update": [{"hold": True}]})
    ui.emit("recording-started", 1)
    ui.settle("check_update", result=UPDATE)
    expect(page.get_by_test_id("update-notice")).not_to_be_visible()
    ui.emit("recording-stopped", 1)
    expect(page.get_by_test_id("update-notice")).not_to_be_visible()
    ui.emit(finish, 1)
    if finish in {"paste-done", "whisper-failed"}:
        expect(page.get_by_test_id("update-notice")).not_to_be_visible()
        assert "update_reminder_shown_at" not in ui.state()["config"]
        # Settle transient completion/error feedback without wall-clock races.
        page.clock.fast_forward(6_000)
    expect(page.get_by_test_id("update-notice")).to_be_visible()


@pytest.mark.parametrize(
    "response",
    [
        {"result": {"available": False, "current_version": "0.3.0"}},
        {"error": "Offline"},
    ],
)
def test_quiet_background_checks_retry_later(app, page, response):
    page.clock.install()
    ui = app(responses={"check_update": [response]})
    expect(page.get_by_test_id("update-notice")).not_to_be_visible()
    expect(page.get_by_role("alert")).not_to_be_visible()
    ui.queue("check_update", {"result": UPDATE})
    page.clock.fast_forward(CHECK_INTERVAL + 1_000)
    expect(page.get_by_test_id("update-notice")).to_be_visible()


def test_old_beta_check_cannot_show_after_channel_changes(app, page):
    ui = app(
        config={"receive_beta_updates": True},
        responses={"check_update": [{"hold": True}]},
    )
    ui.emit("config-updated", {**ui.state()["config"], "receive_beta_updates": False})
    ui.settle("check_update", result={**UPDATE, "version": "0.3.1-beta.1"})
    expect(page.get_by_test_id("update-notice")).not_to_be_visible()


def test_deferred_beta_notice_is_discarded_after_opt_out(app, page):
    ui = app(
        config={"receive_beta_updates": True, "ui_accent": "#e68a3d"},
        responses={"check_update": [{"hold": True}, {"hold": True}]},
    )
    ui.emit("recording-started", 1)
    ui.settle("check_update", result={**UPDATE, "version": "0.3.1-beta.1"})
    expect(page.get_by_test_id("update-notice")).not_to_be_visible()
    stable_config = {**ui.state()["config"], "receive_beta_updates": False}
    # A native config event follows persistence; keep the synthetic disk in sync.
    page.evaluate(
        "config => { window.__sottoTest.state.config = config; }", stable_config
    )
    ui.emit("config-updated", stable_config)
    ui.emit("whisper-empty", 1)
    expect(page.get_by_test_id("update-notice")).not_to_be_visible()
    ui.settle("check_update", result=UPDATE)
    expect(page.get_by_test_id("update-notice")).to_contain_text("0.3.1")
    expect(page.get_by_test_id("update-notice")).not_to_contain_text("beta")


def test_notice_waits_for_whats_new_dialog(app, page):
    ui = app(
        whats_new={
            "version": "0.3.0",
            "notes": "Synthetic notes",
            "url": "https://github.com/stofll/Sotto/releases",
        },
        responses={"check_update": [{"hold": True}]},
    )
    dialog = page.get_by_role("dialog")
    expect(dialog).to_be_visible()
    ui.settle("check_update", result=UPDATE)
    expect(page.get_by_test_id("update-notice")).not_to_be_visible()
    page.keyboard.press("Escape")
    expect(dialog).not_to_be_visible()
    expect(page.get_by_test_id("update-notice")).to_be_visible()


def test_notice_waits_for_confirmation_without_consuming_a_reminder(app, page):
    page.clock.install()
    ui = app(
        config={"ui_accent": "#e68a3d"},
        history=[
            {
                "id": 1,
                "timestamp": 1789200000,
                "text": "Synthetic transcript",
                "length": 20,
            }
        ],
        responses={"check_update": [{"hold": True}]},
    )
    ui.nav("history")
    page.get_by_role("button", name="Очистить всё", exact=True).click()
    dialog = page.get_by_role("alertdialog")
    expect(dialog).to_be_visible()
    ui.settle("check_update", result=UPDATE)
    page.clock.fast_forward(20_000)
    expect(page.get_by_test_id("update-notice")).not_to_be_visible()
    assert "update_reminder_shown_at" not in ui.state()["config"]
    assert not ui.calls("clear_history")
    dialog.get_by_role("button", name="Отмена", exact=True).click()
    expect(page.get_by_test_id("update-notice")).to_be_visible()


def test_whats_new_hidden_by_dictation_keeps_the_reminder_pending(app, page):
    page.clock.install()
    ui = app(
        whats_new=RELEASE,
        responses={"check_update": [{"hold": True}]},
    )
    expect(page.get_by_role("dialog")).to_be_visible()
    ui.settle("check_update", result=UPDATE)
    expect(page.get_by_test_id("update-notice")).not_to_be_visible()
    ui.emit("recording-started", 1)
    expect(page.get_by_role("dialog")).not_to_be_visible()
    page.clock.fast_forward(20_000)
    assert "update_reminder_shown_at" not in ui.state()["config"]
    ui.emit("whisper-empty", 1)
    expect(page.get_by_role("dialog")).to_be_visible()
    expect(page.get_by_test_id("update-notice")).not_to_be_visible()
    assert "update_reminder_shown_at" not in ui.state()["config"]
    page.keyboard.press("Escape")
    expect(page.get_by_role("dialog")).not_to_be_visible()
    expect(page.get_by_test_id("update-notice")).to_be_visible()


def test_reopening_window_reschedules_check_from_the_last_request(app, page):
    page.clock.install()
    ui = app()
    page.wait_for_function(
        "window.__sottoTest.calls.some(call => call.command === 'check_update')"
    )
    page.clock.fast_forward(5 * 60 * 60 * 1000)
    page.evaluate("""() => {
        Object.defineProperty(document, 'visibilityState', {configurable: true, value: 'hidden'});
        document.dispatchEvent(new Event('visibilitychange'));
    }""")
    page.clock.fast_forward(60 * 60 * 1000)
    page.clock.fast_forward(60 * 60 * 1000)
    page.evaluate("""() => {
        Object.defineProperty(document, 'visibilityState', {configurable: true, value: 'visible'});
        document.dispatchEvent(new Event('visibilitychange'));
    }""")
    page.wait_for_function(
        "window.__sottoTest.calls.filter(call => call.command === 'check_update').length === 2"
    )
    page.clock.fast_forward(5 * 60 * 60 * 1000)
    assert len(ui.calls("check_update")) == 2
    ui.queue("check_update", {"result": UPDATE})
    page.clock.fast_forward(60 * 60 * 1000 + 1_000)
    expect(page.get_by_test_id("update-notice")).to_be_visible()
    assert len(ui.calls("check_update")) == 3


@pytest.mark.parametrize(
    "seed",
    [{"window_visible": False}, {"window_minimized": True}, {"window_focused": False}],
)
def test_native_window_inactivity_blocks_checks_even_when_document_is_visible(
    app, page, seed
):
    ui = app(**seed)
    assert page.evaluate("document.visibilityState") == "visible"
    expect(page.get_by_test_id("update-notice")).not_to_be_visible()
    assert not ui.calls("check_update")


def test_native_blur_keeps_delayed_update_pending_until_refocus(app, page):
    page.clock.install()
    ui = app(
        config={"ui_accent": "#e68a3d"}, responses={"check_update": [{"hold": True}]}
    )
    ui.emit("tauri://blur")
    ui.settle("check_update", result=UPDATE)
    page.clock.fast_forward(20_000)
    expect(page.get_by_test_id("update-notice")).not_to_be_visible()
    assert "update_reminder_shown_at" not in ui.state()["config"]
    ui.emit("tauri://focus")
    expect(page.get_by_test_id("update-notice")).to_be_visible()
