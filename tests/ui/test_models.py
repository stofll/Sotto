from pathlib import Path

import pytest
from playwright.sync_api import expect


def test_unknown_meters_use_a_hatch_not_a_dash(app, page):
    ui = app()
    ui.nav("models")
    meters = page.get_by_test_id("model-tiny").locator(".model-meter__unknown")
    expect(meters).to_have_count(2)
    expect(meters.first).to_have_text("")


def test_search_and_reset(app, page):
    ui = app()
    ui.nav("models")
    search = page.get_by_role("searchbox", name="Поиск по названию")
    search.fill("GigaAM")
    expect(page.get_by_test_id("model-gigaam")).to_be_visible()
    expect(page.get_by_test_id("model-tiny")).not_to_be_visible()
    search.fill("missing synthetic model")
    expect(page.get_by_test_id("model-gigaam")).not_to_be_visible()
    search.fill("")
    expect(page.get_by_test_id("model-tiny")).to_be_visible()


def test_select_installed_model_with_keyboard(app, page):
    ui = app()
    ui.nav("models")
    model = page.get_by_test_id("model-gigaam")
    model.focus()
    model.press("Enter")
    page.get_by_role("dialog").get_by_role("button", name="Выбрать", exact=True).click()
    ui.saved("model", "gigaam")
    expect(model).to_have_attribute("aria-pressed", "true")
    page.reload()
    ui.nav("models")
    expect(page.get_by_test_id("model-gigaam")).to_have_attribute(
        "aria-pressed", "true"
    )


def test_download_confirmation_can_cancel(app, page):
    ui = app()
    ui.nav("models")
    page.get_by_test_id("model-base").click()
    dialog = page.get_by_role("dialog", name="Скачать модель?")
    expect(dialog).to_contain_text("Whisper Base")
    page.keyboard.press("Escape")
    expect(dialog).not_to_be_visible()
    assert not ui.calls("download_model")


def test_delete_model_updates_catalog(app, page):
    ui = app()
    ui.nav("models")
    card = page.get_by_test_id("model-gigaam")
    card.get_by_role("button", name="Действия с моделью").click()
    page.get_by_role("menuitem", name="Удалить", exact=True).click()
    page.get_by_role("dialog").get_by_role("button", name="Удалить", exact=True).click()
    page.wait_for_function(
        "window.__sottoTest.state.models.find(m=>m.id==='gigaam').downloaded === false"
    )
    card.click()
    expect(page.get_by_role("dialog", name="Скачать модель?")).to_be_visible()


def test_no_model_banner_navigates_to_catalog(app, page):
    app(
        models=[
            {
                "id": "tiny",
                "label": "Whisper Tiny",
                "size": "75 MB",
                "ram": "400 MB",
                "downloaded": False,
                "selected": True,
            }
        ],
        runtime={"model_loaded": False, "loaded_model": None},
    )
    expect(
        page.get_by_text("Модель распознавания не скачана.", exact=True)
    ).to_be_visible()
    page.get_by_role("button", name="Скачать модель", exact=True).click()
    expect(page.get_by_test_id("page-models")).to_be_visible()


def test_model_selection_failure_preserves_previous(app, page):
    ui = app()
    ui.nav("models")
    ui.queue("set_model", {"error": "Synthetic model load failure"})
    page.get_by_test_id("model-gigaam").click()
    page.get_by_role("dialog").get_by_role("button", name="Выбрать", exact=True).click()
    expect(page.get_by_role("alert")).to_contain_text("Synthetic model load failure")
    expect(page.get_by_test_id("model-tiny")).to_have_attribute("aria-pressed", "true")
    assert ui.state()["config"]["model"] == "tiny"


def test_download_progress_cancel_and_retry(app, page):
    ui = app()
    ui.nav("models")
    ui.queue("download_model", {"hold": True})
    ui.queue("cancel_model_download", {"result": None})
    page.get_by_test_id("model-base").click()
    page.get_by_role("dialog").get_by_role(
        "button", name="Скачать", exact=False
    ).click()
    expect(
        page.get_by_role("button", name="Отменить скачивание", exact=True)
    ).to_be_visible()
    ui.emit(
        "model-download-progress",
        {"model": "base", "percent": 50, "downloaded": 50, "total": 100},
    )
    expect(page.get_by_role("progressbar")).to_have_attribute("aria-valuenow", "50")
    page.get_by_role("button", name="Отменить скачивание", exact=True).click()
    ui.settle("download_model", result=None)
    expect(page.get_by_text("Загрузка отменена", exact=True)).to_be_visible()
    ui.queue("download_model", {"error": "Synthetic network failure"})
    page.get_by_test_id("model-base").click()
    page.get_by_role("dialog").get_by_role(
        "button", name="Скачать", exact=False
    ).click()
    expect(page.get_by_role("alert")).to_contain_text("Synthetic network failure")


def test_download_success_selects_model(app, page):
    ui = app()
    ui.nav("models")
    ui.queue("download_model", {"hold": True})
    page.get_by_test_id("model-base").click()
    page.get_by_role("dialog").get_by_role(
        "button", name="Скачать", exact=False
    ).click()
    # Model availability is a backend output; the test supplies the completed catalog.
    next_models = ui.state()["models"]
    next_models[1]["downloaded"] = True
    ui.queue("list_models", {"result": next_models}, {"result": next_models})
    ui.settle("download_model", result={"downloaded": True})
    expect(page.get_by_role("status")).to_contain_text("Модель скачана и активна")
    ui.saved("model", "base")


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["dark", "light"])
def test_meter_details_only_explain_selected_indicator(
    app, page, locale, theme, output_path
):
    ui = app(
        config={"ui_language": locale, "theme": theme},
        assessments=[
            {
                "id": "tiny",
                "compute": "cpu",
                "load_failed": False,
                "speed": {
                    "score": 0.9,
                    "source": "reference",
                    "samples": 7,
                    "median_ms": 1200,
                    "audio_min": 10,
                    "audio_max": 20,
                    "cold": False,
                    "unstable": False,
                    "reference": "Synthetic CPU",
                },
                "memory": {
                    "score": 0.7,
                    "status": "enough",
                    "required_bytes": 2 * 1024**3,
                    "available_bytes": 8 * 1024**3,
                },
            }
        ],
    )
    ui.nav("models")
    card = page.get_by_test_id("model-tiny")
    speed_name, memory_name = (
        ("Скорость", "Запас памяти") if locale == "ru" else ("Speed", "Memory headroom")
    )
    speed = card.get_by_role("button", name=speed_name + ":")
    memory = card.get_by_role("button", name=memory_name + ":")
    speed.hover()
    tooltip = page.get_by_role("tooltip")
    expect(tooltip).to_have_class("hint-bubble")
    expect(tooltip).to_contain_text("Быстрая" if locale == "ru" else "Fast")
    Path(output_path).mkdir(parents=True, exist_ok=True)
    tooltip.screenshot(path=str(Path(output_path) / "tooltip.png"))
    assert speed.get_attribute("title") is None
    speed.focus()
    speed.press("Escape")
    expect(tooltip).not_to_be_visible()
    speed.press("Enter")
    panel = page.get_by_role("region", name=speed_name, exact=True)
    expect(panel).to_contain_text("Быстрая" if locale == "ru" else "Fast")
    expect(panel).not_to_contain_text("ГБ" if locale == "ru" else "GB")
    expect(panel).not_to_contain_text("Synthetic CPU")
    expect(panel.get_by_role("button")).to_have_count(0)
    Path(output_path).mkdir(parents=True, exist_ok=True)
    panel.screenshot(path=str(Path(output_path) / "speed.png"))
    expect(page.get_by_role("tooltip")).to_have_count(0)
    memory.click()
    panel = page.get_by_role("region", name=memory_name, exact=True)
    expect(panel).to_contain_text("2 ГБ" if locale == "ru" else "2 GB")
    expect(panel).to_contain_text("8 ГБ" if locale == "ru" else "8 GB")
    expect(panel).not_to_contain_text("Быстрая" if locale == "ru" else "Fast")
    expect(panel.get_by_role("button")).to_have_count(0)
    panel.screenshot(path=str(Path(output_path) / "memory.png"))
    page.keyboard.press("Escape")
    expect(panel).not_to_be_visible()
    expect(memory).to_be_focused()
    expect(page.get_by_role("dialog")).to_have_count(0)
    assert not ui.calls("reset_model_assessment")
