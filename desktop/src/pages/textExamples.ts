import { t } from "../i18n";

/** Null means the untouched sample; even an empty user draft must survive locale changes. */
export function textPreview(draft: string | null): string {
  return draft ?? t("эээ ну я я щас обсуждаю тайпскрипт, смайл, потом отправлю мой мейл");
}

export function replacementExamples(): [string, string][] {
  return [
    [t("щас"), t("сейчас")],
    [t("тайпскрипт"), "TypeScript"],
    [t("мой мейл"), "name@example.com"],
    [t("смайл"), ":)"],
  ];
}
