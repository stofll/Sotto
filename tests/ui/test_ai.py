import pytest
from playwright.sync_api import expect

KEYS = {"openai": {"available": True, "label": "Synthetic key", "masked": "test-***"}}
FILE_RESULT = {
    "text": "Synthetic file result",
    "raw_text": "synthetic raw file",
    "formatted_text": "Synthetic file result",
    "ai_status": None,
    "audio_seconds": 3,
    "inference_time_ms": 20,
    "language": "en",
}

PROMPT_PROFILES = [
    {
        "id": name,
        "name": "Profile " + name,
        "provider": "openai",
        "model": "model-" + name,
        "api_key_ref": "openai",
        "prompt_preset": "plain",
        "system_prompt": "Prompt " + name,
    }
    for name in ["A", "B"]
]


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["light", "dark"])
def test_complete_prompt_editor(app, page, locale, theme):
    page.set_viewport_size({"width": 1200, "height": 900})
    ui = app(
        config={
            "ui_language": locale,
            "theme": theme,
            "ai_processing": {
                "profiles": [{**PROMPT_PROFILES[0], "system_prompt": ""}],
                "active_profile_id": "A",
            },
        },
        keys=KEYS,
    )
    ui.nav("ai")
    prompt = page.get_by_role(
        "textbox",
        name="Системный промпт" if locale == "ru" else "System prompt",
        exact=True,
    )
    save = page.get_by_role(
        "button",
        name="Сохранить промпт" if locale == "ru" else "Save prompt",
        exact=True,
    )
    expect(prompt).to_be_editable()
    builtin = prompt.input_value()
    assert "Preserve the source language" in builtin
    assert "return the source text unchanged" in builtin
    assert not any("\u0400" <= char <= "\u04ff" for char in builtin)
    expect(page.locator("textarea[readonly]")).to_have_count(0)
    prompt.focus()
    prompt.press("Tab")
    expect(save).to_be_focused()
    prompt.fill("")
    expect(save).to_be_disabled()
    custom = "Follow these custom instructions only.\nPreserve every sentence."
    prompt.fill(custom)
    save.click()
    page.wait_for_function(
        "expected => window.__sottoTest.state.config.ai_processing.system_prompt === expected",
        arg=custom,
    )
    page.reload()
    ui.nav("ai")
    expect(prompt).to_have_value(custom)
    page.get_by_role(
        "button",
        name="Вернуть встроенный" if locale == "ru" else "Restore the built-in",
        exact=True,
    ).last.click()
    expect(prompt).to_have_value(builtin)
    assert ui.state()["config"]["ai_processing"]["profiles"][0]["system_prompt"] == ""


@pytest.mark.parametrize("custom", [False, True])
def test_startup_refreshes_only_a_stale_built_in_prompt(app, page, custom):
    profile = {**PROMPT_PROFILES[0], "system_prompt": "Prompt A" if custom else ""}
    ui = app(
        config={
            "ai_processing": {
                "profiles": [profile],
                "active_profile_id": "A",
                "system_prompt": "Prompt A" if custom else "Old built-in prompt.",
            },
        },
        keys=KEYS,
    )
    ui.nav("ai")
    shown = page.get_by_role(
        "textbox", name="Системный промпт", exact=True
    ).input_value()
    assert (shown == "Prompt A") == custom
    page.wait_for_function(
        "expected => window.__sottoTest.state.config.ai_processing.system_prompt === expected",
        arg=shown,
    )


@pytest.mark.parametrize("failure", [False, True])
def test_profile_prompt_resets_after_unrelated_save(app, page, failure):
    ui = app(
        config={
            "ai_processing": {"profiles": PROMPT_PROFILES, "active_profile_id": "A"}
        }
    )
    ui.nav("ai")
    prompt = page.locator("textarea").first
    expect(prompt).to_have_value("Prompt A")
    prompt.fill("Unsaved draft A")
    if failure:
        ui.queue("save_config", {"error": "Synthetic disk full"})
    page.get_by_role("radiogroup", name="Режим обработки").get_by_role("radio").nth(
        1
    ).click()
    if failure:
        expect(page.get_by_text("Synthetic disk full", exact=True)).to_be_visible()
    else:
        page.wait_for_function(
            "window.__sottoTest.state.config.ai_processing.pipeline_mode === 'hybrid'"
        )
    expect(prompt).to_have_value("Unsaved draft A")
    page.locator(".active-profile__pick button").click()
    page.get_by_role("option", name="Profile B", exact=False).click()
    expect(prompt).to_have_value("Prompt B")
    assert ui.state()["config"]["ai_processing"]["profiles"][0]["system_prompt"] == (
        "Prompt A"
    )


@pytest.mark.parametrize("action", ["save", "reset"])
def test_prompt_save_failure_preserves_draft_and_can_retry(app, page, action):
    ui = app(
        config={
            "ai_processing": {"profiles": PROMPT_PROFILES, "active_profile_id": "A"}
        }
    )
    ui.nav("ai")
    prompt = page.locator("textarea").first
    prompt.fill("Edited prompt A")
    button = (
        page.get_by_role("button", name="Сохранить промпт", exact=True)
        if action == "save"
        else page.get_by_role("button", name="Вернуть встроенный", exact=True).last
    )
    success = (
        "Системный промпт сохранён."
        if action == "save"
        else "Профиль снова использует встроенный промпт."
    )
    ui.queue("save_config", {"hold": True})
    button.click()
    expect(button).to_be_disabled()
    expect(prompt).to_be_disabled()
    expect(page.locator(".active-profile__pick button")).to_be_disabled()
    ui.settle("save_config", error="Synthetic disk full")
    expect(page.get_by_text("Synthetic disk full", exact=True)).to_be_visible()
    expect(page.get_by_text(success, exact=True)).not_to_be_visible()
    expect(prompt).to_have_value("Edited prompt A")
    expect(button).to_be_enabled()
    assert ui.state()["config"]["ai_processing"]["profiles"][0]["system_prompt"] == (
        "Prompt A"
    )
    button.click()
    expect(page.get_by_text(success, exact=True)).to_be_visible()
    expected = "Edited prompt A" if action == "save" else ""
    assert ui.state()["config"]["ai_processing"]["profiles"][0]["system_prompt"] == (
        expected
    )
    if action == "save":
        expect(prompt).to_have_value(expected)
    else:
        expect(prompt).not_to_have_value("Edited prompt A")
    page.reload()
    ui.nav("ai")
    if action == "save":
        expect(prompt).to_have_value(expected)
    else:
        expect(prompt).not_to_have_value("Prompt A")


def test_failed_profile_switch_keeps_draft_and_does_not_claim_success(app, page):
    ui = app(
        config={
            "ai_processing": {"profiles": PROMPT_PROFILES, "active_profile_id": "A"}
        }
    )
    ui.nav("ai")
    prompt = page.locator("textarea").first
    prompt.fill("Unsaved draft A")
    ui.queue("save_config", {"error": "Synthetic profile save failure"})
    page.locator(".active-profile__pick button").click()
    page.get_by_role("option", name="Profile B", exact=False).click()
    expect(
        page.get_by_text("Synthetic profile save failure", exact=True)
    ).to_be_visible()
    expect(page.locator(".active-profile__pick")).to_contain_text("Profile A")
    expect(prompt).to_have_value("Unsaved draft A")
    expect(
        page.get_by_text("Активный профиль: «Profile B».", exact=True)
    ).not_to_be_visible()
    assert ui.state()["config"]["ai_processing"]["active_profile_id"] == "A"
    page.locator(".active-profile__pick button").click()
    page.get_by_role("option", name="Profile B", exact=False).click()
    expect(prompt).to_have_value("Prompt B")
    expect(
        page.get_by_text("Активный профиль: «Profile B».", exact=True)
    ).to_be_visible()


def test_pipeline_keyboard_selection(app, page):
    ui = app()
    ui.nav("ai")
    modes = page.get_by_role("radiogroup", name="Режим обработки")
    local = modes.get_by_role("radio").nth(0)
    local.focus()
    local.press("ArrowRight")
    expect(modes.get_by_role("radio").nth(1)).to_be_checked()
    expect(modes.get_by_role("radio").nth(1)).to_be_focused()
    page.wait_for_function(
        "window.__sottoTest.state.config.ai_processing.pipeline_mode === 'hybrid'"
    )
    modes.get_by_role("radio").nth(1).press("ArrowRight")
    expect(modes.get_by_role("radio").nth(2)).to_be_checked()


def test_manual_processing_requires_key(app, page):
    ui = app()
    ui.nav("ai")
    page.get_by_placeholder("Вставьте текст для обработки через выбранную LLM").fill(
        "Synthetic input"
    )
    expect(page.get_by_role("button", name="Обработать", exact=True)).to_be_disabled()
    expect(page.get_by_role("alert")).to_contain_text("API-ключа")


@pytest.mark.parametrize(
    "answer,expected",
    [
        (
            {"result": {"available": True, "output": "Synthetic polished output"}},
            "Synthetic polished output",
        ),
        (
            {
                "result": {
                    "available": True,
                    "fallback": True,
                    "output": "Original input",
                    "provider_error": "Synthetic rate limit",
                }
            },
            "Synthetic rate limit",
        ),
        ({"error": "Synthetic LLM timeout"}, "Synthetic LLM timeout"),
    ],
)
def test_manual_processing_results(app, page, answer, expected):
    ui = app(keys=KEYS)
    ui.nav("ai")
    ui.queue("process_text_ai", {"hold": True})
    page.get_by_placeholder("Вставьте текст для обработки через выбранную LLM").fill(
        "Synthetic input"
    )
    page.get_by_role("button", name="Обработать", exact=True).click()
    expect(page.get_by_role("button", name="Обрабатываю…", exact=True)).to_be_disabled()
    ui.settle("process_text_ai", **answer)
    expect(page.get_by_test_id("page-ai")).to_contain_text(expected)
    expect(page.get_by_role("button", name="Обработать", exact=True)).to_be_enabled()


def test_file_picker_cancel_does_not_start_transcription(app, page):
    ui = app()
    ui.nav("ai")
    page.get_by_role("button", name="Выбрать файл").click()
    expect(page.get_by_role("button", name="Выбрать файл")).to_be_enabled()
    assert not ui.calls("transcribe_audio_file")


@pytest.mark.parametrize("failure", [False, True])
def test_file_transcription_loading_result_and_retry(app, page, failure):
    ui = app()
    ui.nav("ai")
    ui.queue("pick_audio_file", {"result": "/synthetic/sample.wav"})
    ui.queue("transcribe_audio_file", {"hold": True})
    page.get_by_role("button", name="Выбрать файл").click()
    expect(page.get_by_role("button", name="Отменить", exact=True)).to_be_visible()
    ui.emit("file-transcription-started", {"session_id": 7})
    expect(page.get_by_role("button", name="Отменить", exact=True)).to_be_visible()
    ui.settle(
        "transcribe_audio_file",
        **(
            {"error": "Synthetic decode failure"}
            if failure
            else {"result": FILE_RESULT}
        ),
    )
    expect(page.get_by_test_id("page-ai")).to_contain_text(
        "Synthetic decode failure" if failure else "Synthetic file result"
    )
    expect(page.get_by_role("button", name="Выбрать файл")).to_be_enabled()
    assert ui.state()["history"] == []
    if failure:
        ui.queue("pick_audio_file", {"result": "/synthetic/sample.wav"})
        ui.queue("transcribe_audio_file", {"result": FILE_RESULT})
        page.get_by_role("button", name="Выбрать файл").click()
        expect(page.get_by_text("Synthetic file result", exact=True)).to_be_visible()
        expect(
            page.get_by_text("Synthetic decode failure", exact=False)
        ).not_to_be_visible()


def test_file_result_shows_stages_and_processes_with_llm_in_place(app, page):
    ui = app(keys=KEYS)
    ui.nav("ai")
    skipped = {
        **FILE_RESULT,
        "ai_status": {
            "enabled": True,
            "attempted": False,
            "used": False,
            "skipped_reason": "duration_below_threshold",
            "min_duration_seconds": 60,
        },
    }
    ui.queue("pick_audio_file", {"result": "/synthetic/sample.wav"})
    ui.queue("transcribe_audio_file", {"result": skipped})
    page.get_by_role("button", name="Выбрать файл").click()
    panel = page.get_by_test_id("page-ai")
    details = panel.get_by_role("button", name="Подробнее", exact=True)
    expect(details).to_have_attribute("aria-expanded", "false")
    details.click()
    # The skip reason is a tile, not the raw backend code.
    expect(panel.get_by_text("LLM · < 60 с", exact=True)).to_be_visible()
    expect(panel.get_by_text("duration_below_threshold")).to_have_count(0)
    panel.get_by_role("button", name="Распознавание", exact=True).click()
    expect(panel).to_contain_text("synthetic raw file")

    ui.queue(
        "process_text_ai",
        {"result": {"available": True, "output": "Synthetic polished file"}},
    )
    panel.get_by_role("button", name="Обработать через LLM", exact=True).click()
    expect(panel.get_by_text("Synthetic polished file", exact=True)).to_be_visible()
    assert ui.calls("process_text_ai")[-1]["args"]["text"] == "Synthetic file result"
    expect(panel.get_by_role("button", name="LLM", exact=True)).to_have_attribute(
        "aria-pressed", "true"
    )
    expect(
        panel.get_by_role("button", name="Обработать через LLM", exact=True)
    ).to_have_count(0)

    # A new file starts folded, whatever the previous one was left at.
    ui.queue("pick_audio_file", {"result": "/synthetic/second.wav"})
    ui.queue("transcribe_audio_file", {"result": FILE_RESULT})
    page.get_by_role("button", name="Выбрать файл").click()
    expect(panel.get_by_role("button", name="Подробнее", exact=True)).to_have_attribute(
        "aria-expanded", "false"
    )


def test_file_cancellation(app, page):
    ui = app()
    ui.nav("ai")
    ui.queue("pick_audio_file", {"result": "/synthetic/sample.wav"})
    ui.queue("transcribe_audio_file", {"hold": True})
    ui.queue("cancel_audio_file", {"result": True})
    page.get_by_role("button", name="Выбрать файл").click()
    ui.emit("file-transcription-started", {"session_id": 23})
    page.get_by_role("button", name="Отменить", exact=True).click()
    page.wait_for_function(
        "window.__sottoTest.calls.some(x=>x.command==='cancel_audio_file')"
    )
    assert ui.calls("cancel_audio_file")[-1]["args"]["session_id"] == 23
    ui.settle("transcribe_audio_file", error="cancelled")
    expect(page.get_by_role("button", name="Выбрать файл")).to_be_enabled()
    expect(page.get_by_text("Synthetic file result", exact=True)).not_to_be_visible()


@pytest.mark.parametrize("cancel_before_event", [False, True])
def test_file_can_cancel_during_decode(app, page, cancel_before_event):
    ui = app()
    ui.nav("ai")
    ui.queue("pick_audio_file", {"result": "/synthetic/sample.wav"})
    ui.queue("transcribe_audio_file", {"hold": True})
    page.get_by_role("button", name="Выбрать файл").click()
    page.wait_for_function(
        "window.__sottoTest.calls.some(x=>x.command==='transcribe_audio_file')"
    )
    if cancel_before_event:
        page.get_by_role("button", name="Отменить", exact=True).click()
    ui.emit("file-transcription-started", {"session_id": 31, "stage": "decoding"})
    if not cancel_before_event:
        expect(page.get_by_text("Читаю файл…", exact=True)).to_be_visible()
        page.get_by_role("button", name="Отменить", exact=True).click()
    page.wait_for_function(
        "window.__sottoTest.calls.some(x=>x.command==='cancel_audio_file')"
    )
    assert ui.calls("cancel_audio_file")[-1]["args"]["session_id"] == 31
    ui.settle("transcribe_audio_file", error="cancelled")
    expect(page.get_by_role("button", name="Выбрать файл")).to_be_enabled()


@pytest.mark.parametrize("early_event", [False, True])
def test_leaving_ai_cancels_file_session(app, page, early_event):
    ui = app()
    ui.nav("ai")
    ui.queue("pick_audio_file", {"result": "/synthetic/sample.wav"})
    ui.queue("transcribe_audio_file", {"hold": True})
    page.get_by_role("button", name="Выбрать файл").click()
    page.wait_for_function(
        "window.__sottoTest.calls.some(x=>x.command==='transcribe_audio_file')"
    )
    if early_event:
        ui.emit("file-transcription-started", {"session_id": 32, "stage": "decoding"})
    ui.nav("settings")
    if not early_event:
        ui.emit("file-transcription-started", {"session_id": 32, "stage": "decoding"})
    page.wait_for_function(
        "window.__sottoTest.calls.some(x=>x.command==='cancel_audio_file')"
    )
    assert ui.calls("cancel_audio_file")[-1]["args"]["session_id"] == 32
    ui.settle("transcribe_audio_file", error="cancelled")
    ui.nav("ai")
    expect(page.get_by_role("button", name="Выбрать файл")).to_be_enabled()


def test_file_subscription_failure_allows_retry(app, page):
    ui = app()
    ui.nav("ai")
    page.evaluate("""() => {
      const internals = window.__TAURI_INTERNALS__;
      const original = internals.invoke;
      internals.invoke = (command, args) => {
        if (command === 'plugin:event|listen' && args.event === 'file-transcription-started') {
          internals.invoke = original;
          return Promise.reject(new Error('Synthetic subscription failure'));
        }
        return original(command, args);
      };
    }""")
    ui.queue("pick_audio_file", {"result": "/synthetic/sample.wav"})
    page.get_by_role("button", name="Выбрать файл").click()
    expect(
        page.get_by_text("Synthetic subscription failure", exact=True)
    ).to_be_visible()
    expect(page.get_by_role("button", name="Выбрать файл")).to_be_enabled()
    assert not ui.calls("transcribe_audio_file")
    ui.queue("pick_audio_file", {"result": "/synthetic/sample.wav"})
    ui.queue("transcribe_audio_file", {"result": FILE_RESULT})
    page.get_by_role("button", name="Выбрать файл").click()
    expect(page.get_by_text("Synthetic file result", exact=True)).to_be_visible()


def test_late_answer_mode_is_saved_for_all_profiles(app, page):
    ui = app(
        config={
            "ai_processing": {"profiles": PROMPT_PROFILES, "active_profile_id": "A"}
        }
    )
    ui.nav("ai")
    page.get_by_role("button", name="Дополнительно", exact=True).click()
    cell = page.locator(".route-advanced__cell").filter(has_text="Поздний ответ LLM")
    cell.get_by_role("button").click()
    page.get_by_role("option", name="Не ждать", exact=True).click()
    page.wait_for_function(
        "window.__sottoTest.state.config.ai_processing.llm_late_answer === 'off'"
    )


def test_reasoning_and_answer_limit_are_saved_on_the_profile(app, page):
    ui = app(
        config={
            "ai_processing": {"profiles": PROMPT_PROFILES, "active_profile_id": "A"}
        }
    )
    ui.nav("ai")
    page.get_by_role("button", name="Дополнительно", exact=True).click()
    reasoning = page.locator(".route-advanced__cell").filter(has_text="Рассуждения")
    reasoning.get_by_role("button").click()
    page.get_by_role("option", name="Как у модели", exact=True).click()
    page.wait_for_function(
        "(() => { const ai = window.__sottoTest.state.config.ai_processing;"
        " return ai.llm_reasoning === 'model'"
        " && ai.profiles.find((p) => p.id === 'A').llm_reasoning === 'model'"
        " && ai.profiles.find((p) => p.id === 'B').llm_reasoning !== 'model'; })()"
    )
    limit = page.locator(".route-advanced__cell").filter(has_text="Лимит ответа")
    expect(limit.get_by_label("Лимит ответа в токенах")).to_have_count(0)
    limit.get_by_role("button").click()
    page.get_by_role("option", name="Своё значение", exact=True).click()
    field = limit.get_by_label("Лимит ответа в токенах")
    expect(field).to_have_value("8192")
    page.wait_for_function(
        "window.__sottoTest.state.config.ai_processing.llm_output_limit === 8192"
    )
