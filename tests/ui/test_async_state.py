"""Regressions for delayed IPC replies in the settings and overlay windows."""

from playwright.sync_api import expect

PROFILE = {
    "id": "synthetic",
    "name": "Synthetic profile",
    "provider": "openai",
    "model": "old-model",
    "api_key_ref": "openai",
    "system_prompt": "Synthetic prompt",
}


def test_ai_changes_are_written_in_order_without_stale_fields(app, page):
    ui = app(
        config={
            "ai_processing": {"profiles": [PROFILE], "active_profile_id": "synthetic"}
        }
    )
    ui.nav("ai")
    page.evaluate("""() => {
      const original = window.__TAURI_INTERNALS__.invoke;
      window.delayedSaves = [];
      window.__TAURI_INTERNALS__.invoke = (command, args) => {
        if (command !== 'save_config') return original(command, args);
        const call = { args, finish: null };
        window.delayedSaves.push(call);
        if (window.delayedSaves.length === 1) {
          return new Promise(resolve => { call.finish = () => resolve(original(command, args)); });
        }
        return original(command, args);
      };
    }""")
    page.get_by_role("radiogroup", name="Режим обработки").get_by_role("radio").nth(
        1
    ).click()
    page.wait_for_function("window.delayedSaves.length === 1")
    page.get_by_role("button", name="Дополнительно", exact=True).click()
    page.locator(".route-advanced__cell").nth(1).locator("input").fill("20")
    # The second request may only start after the first result updates the base.
    assert page.evaluate("window.delayedSaves.length") == 1
    page.evaluate("window.delayedSaves[0].finish()")
    page.wait_for_function("window.delayedSaves.length === 2")
    patches = page.evaluate("window.delayedSaves.map(x => x.args.patch.ai_processing)")
    assert patches[0]["pipeline_mode"] == "hybrid"
    assert set(patches[1]) == {"llm_timeout_seconds", "profiles"}
    assert patches[1]["llm_timeout_seconds"] == 20
    assert patches[1]["profiles"][0]["llm_timeout_seconds"] == 20
    page.wait_for_function("""() => window.__sottoTest.state.config.ai_processing.pipeline_mode === 'hybrid'
      && window.__sottoTest.state.config.ai_processing.llm_timeout_seconds === 20""")


def test_config_notifications_cannot_reorder_writes(app, page):
    ui = app(
        config={
            "ai_processing": {"profiles": [PROFILE], "active_profile_id": "synthetic"}
        }
    )
    ui.nav("ai")
    before = len(ui.calls("save_config"))
    page.evaluate("""() => {
      const original = window.__TAURI_INTERNALS__.invoke;
      window.configEvents = [];
      window.__TAURI_INTERNALS__.invoke = (command, args) => {
        if (command !== 'plugin:event|emit' || args.event !== 'config-updated') return original(command, args);
        window.configEvents.push(args.payload);
        if (window.configEvents.length === 1) {
          return new Promise(resolve => { window.finishFirstEvent = () => resolve(original(command, args)); });
        }
        return original(command, args);
      };
    }""")
    page.get_by_role("radiogroup", name="Режим обработки").get_by_role("radio").nth(
        1
    ).click()
    page.wait_for_function("window.configEvents.length === 1")
    page.get_by_role("button", name="Дополнительно", exact=True).click()
    page.locator(".route-advanced__cell").nth(1).locator("input").fill("20")
    assert len(ui.calls("save_config")) == before + 1
    page.evaluate("window.finishFirstEvent()")
    page.wait_for_function("window.configEvents.length === 2")
    assert page.evaluate(
        "window.configEvents.map(x => x.ai_processing.pipeline_mode)"
    ) == ["hybrid", "hybrid"]
    expect(
        page.get_by_role("radiogroup", name="Режим обработки")
        .get_by_role("radio")
        .nth(1)
    ).to_have_attribute("aria-checked", "true")
    assert ui.state()["config"]["ai_processing"]["llm_timeout_seconds"] == 20


def test_profile_save_failure_keeps_rename_for_retry(app, page):
    ui = app(
        config={
            "ai_processing": {"profiles": [PROFILE], "active_profile_id": "synthetic"}
        }
    )
    ui.nav("integrations")
    row = page.get_by_test_id("profile-synthetic")
    row.get_by_role("button", name="Действия с профилем", exact=True).click()
    page.get_by_role("menuitem", name="Переименовать", exact=True).click()
    field = row.get_by_role("textbox", name="Название профиля")
    field.fill("New profile name")
    ui.queue("save_config", {"error": "Synthetic disk failure"})
    field.press("Enter")
    expect(
        page.get_by_text(
            "Не удалось сохранить профиль. Проверьте ошибку и повторите попытку.",
            exact=True,
        )
    ).to_be_visible()
    expect(page.get_by_text("Профиль переименован.", exact=True)).to_have_count(0)
    expect(field).to_have_value("New profile name")
    assert (
        ui.state()["config"]["ai_processing"]["profiles"][0]["name"]
        == "Synthetic profile"
    )
    field.press("Enter")
    expect(page.get_by_text("Профиль переименован.", exact=True)).to_be_visible()
    assert (
        ui.state()["config"]["ai_processing"]["profiles"][0]["name"]
        == "New profile name"
    )


def test_previous_provider_reply_cannot_replace_current_models(app, page):
    ui = app(
        config={
            "ai_processing": {"profiles": [PROFILE], "active_profile_id": "synthetic"}
        }
    )
    ui.nav("integrations")
    row = page.get_by_test_id("profile-synthetic")
    row.get_by_role("button", name="Synthetic profile", exact=False).click()
    page.evaluate("""() => {
      const original = window.__TAURI_INTERNALS__.invoke;
      window.delayedModels = [];
      window.__TAURI_INTERNALS__.invoke = (command, args) => {
        if (command === 'fetch_provider_models') return new Promise((resolve, reject) => window.delayedModels.push({ args, resolve, reject }));
        return original(command, args);
      };
    }""")
    row.get_by_role("button", name="Запросить список моделей у провайдера").click()
    page.wait_for_function("window.delayedModels.length === 1")
    row.get_by_text("Провайдер", exact=True).locator("..").get_by_role("button").click()
    page.get_by_role("option", name="Anthropic", exact=True).click()
    page.wait_for_function("window.delayedModels.length === 2")
    page.evaluate("window.delayedModels[1].resolve(['new-provider-only'])")
    row.get_by_role("textbox").click()
    expect(page.get_by_role("option", name="new-provider-only")).to_be_visible()
    page.evaluate("window.delayedModels[0].reject(new Error('Old provider failure'))")
    expect(page.get_by_role("option", name="new-provider-only")).to_be_visible()
    expect(page.get_by_text("Old provider failure", exact=False)).to_have_count(0)


OLD_ENTRY = {
    "id": 1,
    "timestamp": 1789200000,
    "text": "Old snapshot entry",
    "raw_text": "old",
    "length": 18,
}
NEW_ENTRY = {
    "id": 2,
    "timestamp": 1789200060,
    "text": "Fresh snapshot entry",
    "raw_text": "fresh",
    "length": 20,
}


def test_old_history_reply_cannot_replace_fresh_snapshot(app, page):
    ui = app()
    page.evaluate("""() => {
      const original = window.__TAURI_INTERNALS__.invoke;
      window.delayedHistory = [];
      window.__TAURI_INTERNALS__.invoke = (command, args) => {
        if (command === 'list_history') return new Promise(resolve => window.delayedHistory.push(resolve));
        return original(command, args);
      };
    }""")
    ui.nav("history")
    page.wait_for_function("window.delayedHistory.length >= 1")
    before = page.evaluate("window.delayedHistory.length")
    page.evaluate("""() => {
      Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'visible' });
      document.dispatchEvent(new Event('visibilitychange'));
    }""")
    page.wait_for_function("n => window.delayedHistory.length > n", arg=before)
    page.evaluate(
        "entry => window.delayedHistory[window.delayedHistory.length - 1]({ entries: [entry], max_age_seconds: 2592000, max_entries: 1000 })",
        NEW_ENTRY,
    )
    expect(page.get_by_test_id("history-entry-2")).to_be_visible()
    page.evaluate(
        "entry => window.delayedHistory[0]({ entries: [entry], max_age_seconds: 2592000, max_entries: 1000 })",
        OLD_ENTRY,
    )
    expect(page.get_by_test_id("history-entry-2")).to_be_visible()
    expect(page.get_by_test_id("history-entry-1")).to_have_count(0)


def test_overlay_live_event_wins_over_initial_snapshot(app, page):
    ui = app("overlay", responses={"current_state": [{"hold": True}]})
    page.wait_for_function("window.__sottoTest.pending('current_state')")
    ui.emit("recording-started", 42)
    ui.emit("overlay-state", "recording")
    expect(page.get_by_test_id("overlay")).to_have_attribute("data-state", "recording")
    ui.settle("current_state", result="processing")
    expect(page.get_by_test_id("overlay")).to_have_attribute("data-state", "recording")
