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
    expect(page.get_by_role("button", name="Выбрать файл")).to_be_disabled()
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
