import type { ModelAssessment } from "../bridge/modelAssessments";
import { t } from "../i18n";

export function speedPresentation(score: number | null | undefined) {
  if (typeof score !== "number" || !Number.isFinite(score)) {
    return { fill: null, label: t("Нет замера"), hint: t("Пока нет сравнительного замера для этой модели.") };
  }
  const fill = score >= 0.8 ? 100 : score >= 0.5 ? 200 / 3 : 100 / 3;
  const label = score >= 0.8 ? t("Высокая") : score >= 0.5 ? t("Средняя") : t("Низкая");
  const description = score >= 0.8 ? t("Высокая относительная скорость.") : score >= 0.5 ? t("Средняя относительная скорость.") : t("Низкая относительная скорость.");
  return { fill, label, hint: `${description} ${t("Оценка по сравнительным тестам; на вашем компьютере скорость может отличаться.")}` };
}

export function downloadSpaceText(value?: ModelAssessment): string {
  const disk = value?.download;
  if (disk?.required_bytes == null || disk.available_bytes == null) return "";
  return t("Для скачивания нужно {p0} ГБ на диске, свободно {p1} ГБ. Освободите место и вернитесь в окно приложения.", {
    p0: String(Math.ceil(disk.required_bytes / 1024 ** 3 * 100) / 100),
    p1: String(Math.floor(disk.available_bytes / 1024 ** 3 * 100) / 100),
  });
}
