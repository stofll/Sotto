"""First-run UI with synthetic configuration; no native data or model downloads."""

from pathlib import Path

import pytest
from playwright.sync_api import expect

MODELS = [
    {
        "id": model_id,
        "label": label,
        "size": size,
        "ram": "1 GB",
        "downloaded": False,
        "selected": model_id == "turbo",
        "engine": engine,
        "languages": languages,
        "streaming": streaming,
    }
    for model_id, label, size, engine, languages, streaming in [
        ("turbo", "Whisper turbo", "834 MB", "whisper.cpp", None, False),
        ("gigaam-v3", "GigaAM v3", "214 MB", "sherpa-onnx", ["ru"], False),
        (
            "nemotron-streaming",
            "Nemotron 3.5",
            "651 MB",
            "sherpa-onnx",
            ["ru", "en"],
            True,
        ),
    ]
]


def first_run(app, step=0, **kwargs):
    config = {
        "onboarding_completed": False,
        "onboarding_step": step,
        "model": "turbo",
        "telemetry_enabled": True,
        **kwargs.pop("config", {}),
    }
    runtime = {
        "model_loaded": False,
        "loaded_model": None,
        "model_loads_on_demand": False,
        **kwargs.pop("runtime", {}),
    }
    return app(
        config=config, runtime=runtime, models=kwargs.pop("models", MODELS), **kwargs
    )


def test_steps_resume_and_skips_never_download_or_change_telemetry(app, page):
    ui = first_run(app)
    expect(page.get_by_test_id("onboarding")).to_be_visible()
    expect(page.get_by_test_id("sidebar")).to_have_count(0)
    page.get_by_role("button", name="Дальше", exact=True).click()
    ui.saved("onboarding_step", 1)
    page.reload()
    expect(
        page.get_by_role("heading", name="Одна модель, чтобы начать")
    ).to_be_visible()
    expect(page.get_by_role("radio", name="Whisper turbo", exact=False)).to_be_checked()
    page.get_by_role("button", name="Пропустить шаг", exact=True).click()
    ui.saved("onboarding_step", 2)
    page.get_by_role("button", name="Пропустить шаг", exact=True).click()
    ui.saved("onboarding_step", 3)
    page.get_by_role("button", name="Пропустить шаг", exact=True).click()
    ui.saved("onboarding_completed", True)
    expect(page.get_by_test_id("page-models")).to_be_visible()
    assert not ui.calls("download_model")
    assert ui.state()["config"]["telemetry_enabled"] is True
    page.reload()
    expect(page.get_by_test_id("page-settings")).to_be_visible()
    expect(page.get_by_test_id("onboarding")).to_have_count(0)


def test_failed_step_save_stays_put_and_can_retry(app, page):
    ui = first_run(app)
    ui.queue("save_config", {"error": "Synthetic disk full"})
    page.get_by_role("button", name="Дальше", exact=True).click()
    expect(
        page.get_by_role("heading", name="Нажали. Сказали. Текст уже там.")
    ).to_be_visible()
    # The reason appears once, next to the step, not also in the window banner.
    expect(page.get_by_role("alert")).to_have_text(
        "Не удалось сохранить настройку: Synthetic disk full"
    )
    page.get_by_role("button", name="Дальше", exact=True).click()
    ui.saved("onboarding_step", 1)
    expect(page.get_by_role("alert")).to_have_count(0)
    expect(
        page.get_by_role("heading", name="Одна модель, чтобы начать")
    ).to_be_visible()


def test_download_continues_after_skip_and_activates_model(app, page):
    ui = first_run(app, 1, responses={"download_model": [{"hold": True}]})
    page.get_by_role("button", name="Скачать и продолжить", exact=True).click()
    ui.saved("onboarding_step", 2)
    page.get_by_role("button", name="Пропустить введение", exact=True).click()
    ui.saved("onboarding_completed", True)
    expect(page.get_by_test_id("page-settings")).to_be_visible()
    expect(page.get_by_text("Скачивается Whisper turbo", exact=True)).to_be_visible()
    ui.emit(
        "model-download-progress", {"model": "turbo", "downloaded": 50, "total": 100}
    )
    expect(page.get_by_role("progressbar", name="Скачивание модели")).to_have_attribute(
        "value", "50"
    )
    page.evaluate("window.__sottoTest.state.models[0].downloaded = true")
    page.evaluate(
        "Object.assign(window.__sottoTest.state.runtime, {model_loaded: true, loaded_model: 'turbo'})"
    )
    ui.settle("download_model", result={"downloaded": True})
    ui.emit("whisper-ready", "turbo")
    expect(page.get_by_text("Можно диктовать", exact=True)).to_be_visible()
    assert ui.calls("set_model")[-1]["args"] == {"model": "turbo"}
    assert len(ui.calls("download_model")) == 1
    page.get_by_role("button", name="Закрыть подсказку", exact=True).click()
    expect(page.get_by_text("Можно диктовать", exact=True)).to_have_count(0)


def test_skipped_download_keeps_models_page_busy_and_warns_after_banner(app, page):
    ui = first_run(app, 1, responses={"download_model": [{"hold": True}]})
    page.get_by_role("button", name="Скачать и продолжить", exact=True).click()
    ui.saved("onboarding_step", 2)
    page.get_by_role("button", name="Пропустить введение", exact=True).click()
    ui.saved("onboarding_completed", True)
    expect(page.locator(".window-banner__copy")).to_be_focused()
    ui.nav("models")
    expect(
        page.get_by_test_id("model-turbo").get_by_role(
            "button", name="Скачать модель", exact=True
        )
    ).to_be_disabled()
    page.get_by_test_id("model-turbo").click()
    expect(page.get_by_role("dialog")).to_have_count(0)
    ui.settle("download_model", error="Synthetic offline")
    expect(page.get_by_text("Модель распознавания не скачана.")).to_have_count(0)
    page.get_by_role("button", name="Закрыть подсказку", exact=True).click()
    # With the card gone, the download's failure and the missing model are still reported.
    expect(
        page.get_by_role("alert").filter(has_text="Synthetic offline")
    ).to_be_visible()
    expect(page.get_by_text("Модель распознавания не скачана.")).to_be_visible()
    assert len(ui.calls("download_model")) == 1


def test_download_failure_can_be_retried_without_leaving_introduction(app, page):
    ui = first_run(
        app, 1, responses={"download_model": [{"error": "Synthetic offline"}]}
    )
    page.get_by_role("button", name="Скачать и продолжить", exact=True).click()
    ui.saved("onboarding_step", 2)
    expect(
        page.get_by_role("alert").filter(has_text="Synthetic offline")
    ).to_be_visible()
    page.get_by_role("button", name="Назад", exact=True).click()
    ui.saved("onboarding_step", 1)
    ui.queue("download_model", {"hold": True})
    page.get_by_role("button", name="Скачать и продолжить", exact=True).click()
    ui.saved("onboarding_step", 2)
    assert len(ui.calls("download_model")) == 2
    ui.queue("cancel_model_download", {"result": None})
    page.get_by_role("button", name="Отменить скачивание", exact=True).click()
    ui.settle("download_model", result=None)
    expect(page.get_by_text("Загрузка отменена", exact=True)).to_be_visible()
    assert ui.state()["config"]["onboarding_completed"] is False


def test_insufficient_space_disables_download_but_allows_skip(app, page):
    first_run(
        app,
        1,
        assessments=[
            {
                "id": "turbo",
                "download": {
                    "insufficient": True,
                    "required_bytes": 2 * 1024**3,
                    "available_bytes": 1024**3,
                },
            }
        ],
    )
    expect(
        page.get_by_role("button", name="Скачать и продолжить", exact=True)
    ).to_be_disabled()
    expect(page.get_by_role("alert")).to_contain_text("свободно 1 ГБ")
    page.get_by_role("button", name="Пропустить шаг", exact=True).click()
    expect(page.get_by_role("heading", name="Ваше сочетание")).to_be_visible()


def test_model_choice_persists_without_starting_download(app, page):
    ui = first_run(app, 1)
    page.get_by_role("radio", name="GigaAM v3", exact=False).check()
    ui.saved("onboarding_model", "gigaam-v3")
    assert ui.state()["config"]["model"] == "turbo"
    page.reload()
    expect(page.get_by_role("radio", name="GigaAM v3", exact=False)).to_be_checked()
    assert not ui.calls("download_model")


def test_downloaded_model_activation_failure_preserves_current_model_and_retries(
    app, page
):
    available = [{**model, "downloaded": True} for model in MODELS]
    ui = first_run(app, 1, models=available)
    page.get_by_role("radio", name="GigaAM v3", exact=False).check()
    ui.saved("onboarding_model", "gigaam-v3")
    ui.queue("set_model", {"error": "Synthetic engine unavailable"})
    page.get_by_role("button", name="Дальше", exact=True).click()
    expect(page.get_by_role("alert")).to_have_count(1)
    expect(page.get_by_role("alert")).to_contain_text("Synthetic engine unavailable")
    assert ui.state()["config"]["model"] == "turbo"
    assert ui.state()["config"]["onboarding_step"] == 1
    page.get_by_role("button", name="Дальше", exact=True).click()
    ui.saved("model", "gigaam-v3")
    ui.saved("onboarding_step", 2)
    assert not ui.calls("download_model")


def test_model_radios_keep_keyboard_focus_and_name_only_the_model(app, page):
    ui = first_run(app, 1)
    turbo = page.get_by_role("radio", name="Whisper turbo", exact=True)
    expect(turbo).to_be_checked()
    turbo.focus()
    page.keyboard.press("ArrowRight")
    ui.saved("onboarding_model", "gigaam-v3")
    page.keyboard.press("ArrowRight")
    ui.saved("onboarding_model", "nemotron-streaming")
    expect(page.get_by_role("radio", name="Nemotron 3.5", exact=True)).to_be_focused()


def test_download_uses_the_choice_even_while_its_save_is_queued(app, page):
    ui = first_run(app, 1, responses={"download_model": [{"hold": True}]})
    ui.queue("save_config", {"hold": True})
    page.get_by_role("radio", name="GigaAM v3", exact=True).check()
    page.get_by_role("button", name="Скачать и продолжить", exact=True).click()
    ui.settle(
        "save_config", result={**ui.state()["config"], "onboarding_model": "gigaam-v3"}
    )
    ui.saved("onboarding_step", 2)
    assert ui.calls("download_model")[0]["args"] == {"model": "gigaam-v3"}
    assert ui.state()["config"]["onboarding_model"] == "gigaam-v3"


def test_replay_sees_a_download_started_on_the_models_page(app, page):
    ui = app(
        config={"model": "turbo"},
        models=MODELS,
        responses={"download_model": [{"hold": True}]},
    )
    ui.nav("models")
    page.get_by_test_id("model-turbo").get_by_role(
        "button", name="Скачать модель", exact=True
    ).click()
    page.wait_for_function("window.__sottoTest.pending('download_model')")
    ui.nav("info")
    page.get_by_role("button", name="Пройти введение ещё раз", exact=True).click()
    page.get_by_role("button", name="Дальше", exact=True).click()
    ui.saved("onboarding_step", 1)
    expect(page.get_by_role("radio", name="Whisper turbo", exact=True)).to_be_disabled()
    page.get_by_role("button", name="Дальше", exact=True).click()
    ui.saved("onboarding_step", 2)
    page.get_by_role("button", name="Пропустить введение", exact=True).click()
    ui.saved("onboarding_completed", True)
    expect(page.get_by_text("Скачивается Whisper turbo", exact=True)).to_be_visible()
    assert len(ui.calls("download_model")) == 1


def test_english_interface_offers_models_for_english(app, page):
    # A fresh installation has no speech language yet.
    first_run(app, 1, config={"ui_language": "en", "language": None})
    expect(page.get_by_role("radio")).to_have_count(2)
    expect(page.get_by_role("radio", name="GigaAM v3", exact=True)).to_have_count(0)


@pytest.mark.parametrize(
    "current, label", [("turbo", "Whisper turbo"), ("large-v3", "Whisper large-v3")]
)
def test_replay_keeps_a_working_model_without_reloading_it(app, page, current, label):
    working = [{**model, "downloaded": model["id"] == "turbo"} for model in MODELS] + [
        {
            **MODELS[0],
            "id": "large-v3",
            "label": "Whisper large-v3",
            "downloaded": True,
            "selected": False,
        }
    ]
    ui = app(
        config={"model": current},
        runtime={"model_loaded": True, "loaded_model": current},
        models=working,
    )
    ui.nav("info")
    page.get_by_role("button", name="Пройти введение ещё раз", exact=True).click()
    ui.saved("onboarding_model", current)
    page.get_by_role("button", name="Дальше", exact=True).click()
    ui.saved("onboarding_step", 1)
    expect(page.get_by_role("radio", name=label, exact=True)).to_be_checked()
    page.get_by_role("button", name="Дальше", exact=True).click()
    ui.saved("onboarding_step", 2)
    assert not ui.calls("set_model")
    assert not ui.calls("download_model")


def test_replay_from_help_and_release_notes_are_deferred(app, page):
    ui = first_run(
        app,
        whats_new={
            "version": "0.0.5-test",
            "notes": "Synthetic notes",
            "url": "https://example.test/notes",
        },
    )
    expect(page.get_by_role("dialog", name="Что нового")).to_have_count(0)
    page.get_by_role("button", name="Пропустить введение", exact=True).click()
    ui.saved("onboarding_completed", True)
    expect(page.get_by_role("dialog", name="Что нового")).to_have_count(0)
    assert not ui.calls("get_whats_new")
    ui.nav("info")
    page.get_by_role("button", name="Пройти введение ещё раз", exact=True).click()
    ui.saved("onboarding_completed", False)
    expect(page.get_by_test_id("onboarding")).to_be_visible()
    ui.saved("onboarding_step", 0)


TELEMETRY = "Разрешить обезличенную телеметрию"
EXPLANATION = "Что отправляет телеметрия"


def telemetry_writes(ui):
    return [
        c for c in ui.calls("save_config") if "telemetry_enabled" in c["args"]["patch"]
    ]


@pytest.mark.parametrize("leave", ["Начать диктовать", "Пропустить шаг"])
def test_last_step_records_an_unticked_telemetry_answer(app, page, leave):
    ui = first_run(app, 3, config={"telemetry_enabled": None})
    expect(page.get_by_role("checkbox", name=TELEMETRY, exact=True)).not_to_be_checked()
    page.get_by_role("button", name=leave, exact=True).click()
    ui.saved("telemetry_enabled", False)
    ui.saved("onboarding_completed", True)


def test_skipped_introduction_leaves_telemetry_unanswered(app, page):
    ui = first_run(app, 1, config={"telemetry_enabled": None})
    page.get_by_role("button", name="Пропустить введение", exact=True).click()
    ui.saved("onboarding_completed", True)
    assert ui.state()["config"]["telemetry_enabled"] is None


@pytest.mark.parametrize("dismiss", ["Отмена", "Закрыть", "escape", "outside"])
def test_telemetry_opt_in_needs_the_explanation(app, page, dismiss):
    ui = first_run(app, 3, config={"telemetry_enabled": False})
    control = page.get_by_role("checkbox", name=TELEMETRY, exact=True)
    control.click()
    dialog = page.get_by_role("dialog", name=EXPLANATION)
    expect(dialog).to_be_visible()
    if dismiss == "escape":
        page.keyboard.press("Escape")
    elif dismiss == "outside":
        page.locator(".modal-overlay").click(position={"x": 2, "y": 2})
    else:
        dialog.get_by_role("button", name=dismiss, exact=True).click()
    expect(dialog).to_have_count(0)
    expect(control).not_to_be_checked()
    assert not telemetry_writes(ui)
    control.click()
    dialog.get_by_role("button", name="Включить", exact=True).click()
    ui.saved("telemetry_enabled", True)
    expect(dialog).to_have_count(0)
    expect(control).to_be_checked()


def test_telemetry_failed_opt_in_retries_and_opt_out_is_immediate(app, page):
    ui = first_run(app, 3, config={"telemetry_enabled": False})
    control = page.get_by_role("checkbox", name=TELEMETRY, exact=True)
    control.click()
    ui.queue("save_config", {"error": "Synthetic disk full"})
    dialog = page.get_by_role("dialog", name=EXPLANATION)
    dialog.get_by_role("button", name="Включить", exact=True).click()
    expect(dialog.get_by_role("alert")).to_have_text(
        "Не удалось сохранить настройку: Synthetic disk full"
    )
    expect(page.get_by_role("alert")).to_have_count(1)
    assert ui.state()["config"]["telemetry_enabled"] is False
    dialog.get_by_role("button", name="Включить", exact=True).click()
    ui.saved("telemetry_enabled", True)
    control.uncheck()
    ui.saved("telemetry_enabled", False)
    expect(dialog).to_have_count(0)


def test_telemetry_dialog_is_shared_with_advanced_settings(app, page):
    ui = app(config={"telemetry_enabled": False})
    page.get_by_text("Дополнительно", exact=True).click()
    page.get_by_role("checkbox", name=TELEMETRY, exact=True).click()
    dialog = page.get_by_role("dialog", name=EXPLANATION)
    expect(dialog).to_be_visible()
    # Styled without the introduction's module ever loading.
    expect(dialog.locator("dd").first).to_have_css("margin-left", "0px")
    dialog.get_by_role("button", name="Включить", exact=True).click()
    ui.saved("telemetry_enabled", True)


def test_unanswered_installation_is_asked_after_a_dictation(app, page):
    ui = app(config={"telemetry_enabled": None})
    expect(page.get_by_test_id("page-settings")).to_be_visible()
    question = page.get_by_text("Помочь улучшить Sotto?", exact=True)
    expect(question).to_have_count(0)
    # Persisted like the harness's own writes, so the reload below keeps it.
    page.evaluate(
        "() => { const s = window.__sottoTest.state; s.stats.total_transcriptions = 1;"
        " sessionStorage.setItem('sotto-test-state', JSON.stringify(s)); }"
    )
    ui.emit("paste-done")
    expect(question).to_be_visible()
    page.get_by_role("button", name="Закрыть подсказку", exact=True).click()
    expect(question).to_have_count(0)
    assert ui.state()["config"]["telemetry_enabled"] is None
    page.reload()
    expect(question).to_be_visible()


@pytest.mark.parametrize(
    "answer,value", [("Разрешить", True), ("Не отправлять", False)]
)
def test_telemetry_question_stores_the_answer_once(app, page, answer, value):
    ui = app(config={"telemetry_enabled": None}, stats={"total_transcriptions": 3})
    question = page.get_by_text("Помочь улучшить Sotto?", exact=True)
    expect(question).to_be_visible()
    ui.queue("save_config", {"error": "Synthetic disk full"})
    page.get_by_role("button", name=answer, exact=True).click()
    expect(page.get_by_role("alert")).to_have_text(
        "Не удалось сохранить настройку: Synthetic disk full"
    )
    page.get_by_role("button", name=answer, exact=True).click()
    ui.saved("telemetry_enabled", value)
    expect(question).to_have_count(0)
    page.reload()
    expect(page.get_by_test_id("page-settings")).to_be_visible()
    expect(question).to_have_count(0)


def test_telemetry_question_explains_before_enabling(app, page):
    ui = app(config={"telemetry_enabled": None}, stats={"total_transcriptions": 1})
    page.get_by_role("button", name="Что отправляется", exact=True).click()
    dialog = page.get_by_role("dialog", name=EXPLANATION)
    dialog.get_by_role("button", name="Отмена", exact=True).click()
    assert not telemetry_writes(ui)
    page.get_by_role("button", name="Что отправляется", exact=True).click()
    dialog.get_by_role("button", name="Включить", exact=True).click()
    ui.saved("telemetry_enabled", True)
    expect(page.get_by_text("Помочь улучшить Sotto?", exact=True)).to_have_count(0)


def test_introduction_hides_the_telemetry_question(app, page):
    first_run(
        app, 0, config={"telemetry_enabled": None}, stats={"total_transcriptions": 2}
    )
    expect(page.get_by_test_id("onboarding")).to_be_visible()
    expect(page.get_by_text("Помочь улучшить Sotto?", exact=True)).to_have_count(0)


@pytest.mark.parametrize("recovery", ["reload", "skip"])
def test_failed_introduction_module_can_reload_or_skip(
    app, page, pytestconfig, recovery
):
    ui = app()
    production = pytestconfig.getoption("--ui-mode") == "production"
    module_url = (
        "**/assets/Onboarding-*.js"
        if production
        else "**/src/onboarding/Onboarding.tsx*"
    )
    ui.allow_asset_failure(module_url)
    page.route(
        module_url,
        lambda route: route.fulfill(
            status=503,
            body="Synthetic unavailable",
            headers={"Cache-Control": "no-store"},
        ),
    )
    ui.nav("info")
    page.get_by_role("button", name="Пройти введение ещё раз", exact=True).click()
    expect(page.get_by_role("alert")).to_contain_text("Не удалось открыть введение.")
    page.unroute(module_url)
    if recovery == "reload":
        page.get_by_role("button", name="Перезагрузить", exact=True).click()
        expect(page.get_by_test_id("onboarding")).to_be_visible()
        assert ui.state()["config"]["onboarding_completed"] is False
    else:
        page.get_by_role("button", name="Пропустить введение", exact=True).click()
        ui.saved("onboarding_completed", True)
        expect(page.get_by_test_id("page-settings")).to_be_visible()


def test_cloud_replay_explains_existing_route_and_skip_preserves_it(app, page):
    ui = first_run(app, config={"ai_processing": {"pipeline_mode": "cloud"}})
    expect(
        page.get_by_text(
            "Сейчас запись отправляется выбранному облачному сервису.", exact=True
        )
    ).to_be_visible()
    page.get_by_role("button", name="Пропустить введение", exact=True).click()
    ui.saved("onboarding_completed", True)
    assert ui.state()["config"]["ai_processing"]["pipeline_mode"] == "cloud"
    assert not ui.calls("download_model")


def test_cloud_route_switches_to_a_downloaded_model_only_when_asked(app, page):
    ui = first_run(
        app,
        1,
        config={"ai_processing": {"pipeline_mode": "cloud"}},
        models=[{**model, "downloaded": model["id"] == "turbo"} for model in MODELS],
    )
    expect(page.get_by_role("button", name="Дальше", exact=True)).to_have_count(0)
    page.get_by_role("button", name="Пропустить шаг", exact=True).click()
    ui.saved("onboarding_step", 2)
    assert ui.state()["config"]["ai_processing"]["pipeline_mode"] == "cloud"
    page.get_by_role("button", name="Назад", exact=True).click()
    ui.saved("onboarding_step", 1)
    page.get_by_role("button", name="Перейти на локальную модель", exact=True).click()
    ui.saved("onboarding_step", 2)
    assert ui.calls("set_model")[-1]["args"] == {"model": "turbo"}
    assert ui.state()["config"]["ai_processing"]["pipeline_mode"] == "local"


def test_explicit_local_model_download_switches_cloud_route(app, page):
    ui = first_run(
        app,
        1,
        config={
            "ai_processing": {"pipeline_mode": "cloud", "model": "synthetic-cloud"}
        },
        responses={"download_model": [{"hold": True}]},
    )
    page.get_by_role(
        "button", name="Скачать и перейти на локальную", exact=True
    ).click()
    ui.saved("onboarding_step", 2)
    assert ui.state()["config"]["ai_processing"]["pipeline_mode"] == "cloud"
    page.evaluate("window.__sottoTest.state.models[0].downloaded = true")
    ui.settle("download_model", result={"downloaded": True})
    page.wait_for_function(
        "window.__sottoTest.state.config.ai_processing.pipeline_mode === 'local'"
    )
    assert ui.state()["config"]["ai_processing"]["model"] == "synthetic-cloud"


def test_motion_pauses_when_window_loses_focus_and_respects_reduced_motion(app, page):
    first_run(app)
    region = page.get_by_test_id("onboarding")
    waves = page.locator(".onboarding__waves path")
    page.bring_to_front()
    page.evaluate("window.dispatchEvent(new FocusEvent('focus'))")
    expect(region).to_have_attribute("data-moving", "true")
    initial = waves.first.evaluate("e => getComputedStyle(e).transform")
    page.wait_for_function(
        "initial => getComputedStyle(document.querySelector('.onboarding__waves path')).transform !== initial",
        arg=initial,
    )
    page.evaluate("window.dispatchEvent(new FocusEvent('blur'))")
    expect(region).to_have_attribute("data-moving", "false")
    for element in [page.locator(".onboarding__speech i").first, *waves.all()]:
        expect(element).to_have_css("animation-play-state", "paused")
    page.evaluate("window.dispatchEvent(new FocusEvent('focus'))")
    expect(region).to_have_attribute("data-moving", "true")
    for wave in waves.all():
        expect(wave).to_have_css("animation-play-state", "running")
    page.emulate_media(reduced_motion="reduce")
    for element in [page.locator(".onboarding__speech i").first, *waves.all()]:
        expect(element).to_have_css("animation-name", "none")


def test_portable_and_macos_controls(app, page):
    ui = first_run(
        app,
        2,
        runtime={"portable": True, "os": "macos"},
        responses={"check_accessibility": [{"result": False}] * 8},
    )
    expect(page.locator(".accessibility-notice")).to_be_visible()
    ui.emit("whisper-loading", "turbo")
    page.get_by_role("button", name="Дальше", exact=True).click()
    expect(
        page.get_by_role("checkbox", name="Запускать вместе с системой", exact=True)
    ).to_have_count(0)


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["dark", "light"])
def test_onboarding_layout_and_keyboard_focus(
    app, page, locale, theme, output_path, tab_key
):
    page.set_viewport_size({"width": 1000, "height": 710})
    first_run(
        app, config={"ui_language": locale, "theme": theme, "telemetry_enabled": False}
    )
    region = page.get_by_test_id("onboarding")
    expect(region.locator(".onboarding__heading p")).to_have_count(0)
    expect(
        region.get_by_role(
            "button",
            name="Пропустить шаг" if locale == "ru" else "Skip step",
            exact=True,
        )
    ).to_have_count(0)
    brand = page.locator(".sidebar-brand__name").bounding_box()
    toolbar = page.locator(".onboarding__toolbar")
    expect(toolbar).to_be_visible()
    for control in toolbar.locator("button").all():
        box = control.bounding_box()
        assert abs(box["y"] + box["height"] / 2 - brand["y"] - brand["height"] / 2) <= 1
        assert box["x"] >= brand["x"] + brand["width"]
    shots = Path(output_path)
    shots.mkdir(parents=True, exist_ok=True)
    for step in range(4):
        heading = region.get_by_role("heading", level=1)
        expect(heading).to_be_focused()
        page.keyboard.press(tab_key)
        focused = page.evaluate("document.activeElement.tagName")
        assert focused in {"BUTTON", "INPUT"}
        page.evaluate("() => document.fonts.ready.then(() => true)")
        overflow = region.evaluate("e => e.scrollWidth - e.clientWidth")
        assert overflow <= 1, f"step {step} overflows by {overflow}px"
        page.screenshot(
            path=str(shots / f"onboarding-{step + 1}.png"), animations="disabled"
        )
        if step < 3:
            region.get_by_role(
                "button",
                name=("Дальше" if locale == "ru" else "Next")
                if step == 0
                else ("Пропустить шаг" if locale == "ru" else "Skip step"),
                exact=True,
            ).click()
    region.get_by_role("checkbox").last.click()
    dialog = page.get_by_role("dialog")
    expect(dialog).to_be_visible()
    page.keyboard.press(tab_key)
    assert dialog.evaluate("e => e.contains(document.activeElement)")
    assert dialog.evaluate("e => e.scrollWidth - e.clientWidth") <= 1
    box = dialog.bounding_box()
    assert box and box["y"] >= 0 and box["y"] + box["height"] <= 710
    expect(dialog.locator(".modal__foot button").last).to_be_in_viewport()
    page.screenshot(path=str(shots / "telemetry.png"), animations="disabled")
