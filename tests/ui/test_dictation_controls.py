import re
from pathlib import Path

import pytest
from playwright.sync_api import expect


def expect_preview_after(page, expected):
    # The preview also renders removed words, struck through in the diff.
    page.wait_for_function(
        "expected => [...document.querySelectorAll('.preview-result > span')]"
        ".filter(span => !span.style.textDecoration.includes('line-through'))"
        ".map(span => span.textContent).join('') === expected",
        arg=expected,
    )


def test_old_settings_keep_new_processing_opt_in(app, page):
    ui = app()
    # Simulate a saved config from before the new processing options existed.
    page.evaluate("""() => {
      const f = window.__sottoTest.state.config.text_formatting;
      delete f.normalize_russian_numbers;
      delete f.normalize_dimensions;
      delete f.punctuation_mode;
    }""")
    ui.emit("config-updated", ui.state()["config"])
    ui.nav("text")
    page.get_by_role("button", name=re.compile(r"^Очистка")).click()
    for label in ["Русские числа — цифрами", "Оформлять размеры"]:
        expect(page.get_by_role("button", name=label, exact=True)).to_have_attribute(
            "aria-pressed", "false"
        )
    expect(page.get_by_role("button", name="Автоматически", exact=True)).to_be_visible()
    expect(
        page.get_by_role("button", name="Заглавные и точка", exact=True)
    ).to_have_attribute("aria-pressed", "true")


def test_number_and_dimension_options_are_independent_and_refresh_preview(app, page):
    ui = app()
    ui.nav("text")
    page.get_by_role("button", name=re.compile(r"^Очистка")).click()
    field = page.get_by_placeholder("Введите текст для проверки обработки")
    original = (
        "размерами тридцать пять на двадцать миллиметров, толщина пять миллиметров"
    )
    field.fill(original)
    preview = page.locator(".preview-result")
    expect(preview).to_have_text(original)
    # The browser verifies the preview contract; native tests cover conversion.
    dimensions = "размерами 35 х 20 мм, толщина пять миллиметров"
    ui.queue(
        "preview_format",
        {
            "result": {
                "original": original,
                "formatted": dimensions,
                "dictionary_matches": [],
            }
        },
    )
    page.get_by_role("button", name="Оформлять размеры", exact=True).click()
    expect_preview_after(page, dimensions)
    formatting = ui.state()["config"]["text_formatting"]
    assert formatting["normalize_dimensions"] is True
    assert formatting["normalize_russian_numbers"] is False

    numbers = "размерами 35 х 20 мм, толщина 5 мм"
    ui.queue(
        "preview_format",
        {
            "result": {
                "original": original,
                "formatted": numbers,
                "dictionary_matches": [],
            }
        },
    )
    page.get_by_role("button", name="Русские числа — цифрами", exact=True).click()
    expect_preview_after(page, numbers)
    formatting = ui.state()["config"]["text_formatting"]
    assert formatting["normalize_dimensions"] is True
    assert formatting["normalize_russian_numbers"] is True
    assert formatting["punctuation_mode"] == "automatic"


@pytest.mark.parametrize("locale", ["ru", "en"])
@pytest.mark.parametrize("theme", ["dark", "light"])
def test_punctuation_policy_preserves_sentence_style_and_recovers_from_failed_save(
    app, page, output_path, locale, theme
):
    page.set_viewport_size({"width": 1000, "height": 710})
    ui = app(
        config={
            "ui_language": locale,
            "theme": theme,
            "text_formatting": {
                "capitalize_sentences": True,
                "final_punctuation": True,
            },
        }
    )
    ui.nav("text")
    labels = (
        [
            "Очистка",
            "Автоматически",
            "Без точек, ? и !",
            "Без знаков препинания",
            "Начинать со строчной",
            "Начинать с заглавной",
            "Заглавные и точка",
        ]
        if locale == "ru"
        else [
            "Cleanup",
            "Automatic",
            "No periods, ? or !",
            "No punctuation",
            "Lowercase start",
            "Capitalized start",
            "Capitals and full stop",
        ]
    )
    cleanup, automatic, no_breaks, none, lowercase, capitalized, original_style = labels
    page.get_by_role("button", name=re.compile("^" + cleanup)).click()
    selector = page.get_by_role("button", name=automatic, exact=True)
    selector.click()
    ui.queue("save_config", {"error": "Synthetic punctuation save failure"})
    page.get_by_role("option", name=no_breaks, exact=True).click()
    expect(page.get_by_role("alert")).to_contain_text(
        "Synthetic punctuation save failure"
    )
    expect(selector).to_be_visible()
    assert ui.state()["config"]["text_formatting"]["punctuation_mode"] == "automatic"
    selector.click()
    page.get_by_role("option", name=no_breaks, exact=True).click()
    selector = page.get_by_role("button", name=no_breaks, exact=True)
    expect(selector).to_be_visible()
    formatting = ui.state()["config"]["text_formatting"]
    assert formatting["punctuation_mode"] == "no_sentence_breaks"
    assert formatting["capitalize_sentences"] is True
    assert formatting["final_punctuation"] is True
    expect(page.get_by_role("button", name=original_style, exact=True)).to_have_count(0)

    page.get_by_role("button", name=lowercase, exact=True).click()
    expect(page.get_by_role("button", name=lowercase, exact=True)).to_have_attribute(
        "aria-pressed", "true"
    )
    assert ui.state()["config"]["text_formatting"]["final_punctuation"] is True
    page.get_by_role("button", name=capitalized, exact=True).click()
    expect(page.get_by_role("button", name=capitalized, exact=True)).to_have_attribute(
        "aria-pressed", "true"
    )
    selector.focus()
    page.keyboard.press("Space")
    option = page.get_by_role("option", name=none, exact=True)
    option.focus()
    page.keyboard.press("Enter")
    selector = page.get_by_role("button", name=none, exact=True)
    expect(selector).to_be_visible()
    assert ui.state()["config"]["text_formatting"]["punctuation_mode"] == "none"

    page.reload()
    ui.nav("text")
    heading = page.get_by_role("button", name=re.compile("^" + cleanup))
    if heading.get_attribute("aria-expanded") != "true":
        heading.click()
    expect(selector).to_be_visible()
    selector.focus()
    expect(selector).to_be_focused()
    assert (
        page.get_by_test_id("main-content").evaluate(
            "e => e.scrollWidth - e.clientWidth"
        )
        <= 1
    )
    Path(output_path).mkdir(parents=True, exist_ok=True)
    page.screenshot(
        path=str(Path(output_path) / f"dictation-controls-{locale}-{theme}.png"),
        animations="disabled",
    )
    selector.click()
    page.get_by_role("option", name=automatic, exact=True).click()
    expect(
        page.get_by_role("button", name=original_style, exact=True)
    ).to_have_attribute("aria-pressed", "true")
