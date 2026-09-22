import type { ModelAssessment } from "../bridge/modelAssessments";
import { t } from "../i18n";

type SpeedPresentation = { fill: number | null; label: string; hint: string };

export function speedPresentation(score: number | null | undefined): SpeedPresentation {
  if (typeof score !== "number" || !Number.isFinite(score)) {
    return { fill: null, label: t("Нет замера"), hint: t("Пока нет сравнительного замера для этой модели.") };
  }
  const caveat = t("Оценка по сравнительным тестам; на вашем компьютере скорость может отличаться.");
  if (score >= 0.8) return { fill: 100, label: t("Высокая"), hint: `${t("Высокая относительная скорость.")} ${caveat}` };
  if (score >= 0.5) return { fill: 200 / 3, label: t("Средняя"), hint: `${t("Средняя относительная скорость.")} ${caveat}` };
  return { fill: 100 / 3, label: t("Низкая"), hint: `${t("Низкая относительная скорость.")} ${caveat}` };
}

export function downloadSpaceText(value?: ModelAssessment): string {
  const disk = value?.download;
  if (disk?.required_bytes == null || disk.available_bytes == null) return "";
  return t("Для скачивания нужно {p0} ГБ на диске, свободно {p1} ГБ. Освободите место и вернитесь в окно приложения.", {
    p0: String(Math.ceil(disk.required_bytes / 1024 ** 3 * 100) / 100),
    p1: String(Math.floor(disk.available_bytes / 1024 ** 3 * 100) / 100),
  });
}
