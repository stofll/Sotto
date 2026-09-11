import type { ModelAssessment } from "../bridge/modelAssessments";
import { t } from "../i18n";

const decimal = (value: number) => Number(value.toFixed(1)).toString();

export function assessmentText(value?: ModelAssessment): { speed: string; memory: string; compute: string } {
  const result = {
    speed: t("Скорость пока неизвестна. Оценка появится после распознаваний или эталонных тестов."),
    memory: t("Не удалось оценить запас памяти."),
    compute: "",
  };
  if (!value) return result;
  result.compute = value.compute === "cpu" ? "CPU" : t("GPU выбран, но фактическое ускорение не подтверждено.");
  const speed = value.speed;
  if (speed.source === "personal" && speed.median_ms !== null && speed.audio_min !== null && speed.audio_max !== null) {
    result.speed = t("На вашем компьютере: {p0}–{p1} с аудио — обычно {p2} с обработки. Записей: {p3}.", {
      p0: decimal(speed.audio_min), p1: decimal(speed.audio_max), p2: decimal(speed.median_ms / 1000), p3: speed.samples,
    });
    if (speed.cold) result.speed += ` ${t("Первые распознавания после загрузки модели.")}`;
    if (speed.unstable) result.speed += ` ${t("Скорость нестабильна; для шкалы пока недостаточно согласованных замеров.")}`;
  } else if (speed.source === "reference") {
    result.speed = t("Сравнительная скорость по эталонным тестам: {p0}. Скорость на вашем компьютере может отличаться.", { p0: speed.reference ?? "CPU" });
  }
  if (speed.samples > 0 && speed.samples < 5) {
    result.speed += ` ${t("Уточняем скорость: {p0} из 5 сопоставимых записей.", { p0: speed.samples })}`;
  }
  const memory = value.memory;
  if (memory.status === "loaded") result.memory = t("Модель уже в памяти. Дополнительная загрузка не требуется; запас для распознавания зависит от длины аудио.");
  else if (memory.status === "gpu_unknown") result.memory = t("Не удалось проверить запас видеопамяти. Это не запрещает запуск модели.");
  else if (memory.status === "low") result.memory = t("Может не хватить памяти с учётом запаса для системы. Можно освободить память или выбрать модель меньше.");
  else if (memory.status === "enough") result.memory = t("Вероятно, памяти достаточно. Больше заполнения — больше свободного запаса.");
  if (memory.required_bytes !== null && memory.available_bytes !== null) {
    result.memory += ` ${t("Оценка RAM: {p0} ГБ; доступно сейчас: {p1} ГБ.", { p0: decimal(memory.required_bytes / 1024 ** 3), p1: decimal(memory.available_bytes / 1024 ** 3) })}`;
  }
  return result;
}

export function meterPercent(score: number | null | undefined): number | null {
  return typeof score === "number" && Number.isFinite(score) ? Math.round(Math.max(0, Math.min(1, score)) * 100) : null;
}
