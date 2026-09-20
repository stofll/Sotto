import type { ModelAssessment } from "../bridge/modelAssessments";
import { t } from "../i18n";

const decimal = (value: number) => Number(value.toFixed(1)).toString();

export function assessmentText(value?: ModelAssessment): { speed: string; memory: string } {
  const result = {
    speed: t("Пока нет оценки скорости для этой модели."),
    memory: t("Пока не удалось определить, хватит ли памяти для этой модели."),
  };
  if (!value) return result;
  const score = value.speed.score;
  if (typeof score === "number" && Number.isFinite(score)) {
    result.speed = score >= 0.8
      ? t("Быстрая: короткое ожидание после записи.")
      : score >= 0.5
        ? t("Умеренная скорость: после записи придётся немного подождать.")
        : t("Медленная: обработка может длиться дольше самой записи.");
    result.speed += ` ${t("Это ориентир: скорость зависит от компьютера и длины записи.")}`;
    // The catalogue is measured on a processor, language by language. Saying
    // which context the number came from is the difference between a hint and
    // a claim about the machine in front of the user.
    if (value.speed.source === "reference" && value.speed.approximate) {
      result.speed += ` ${t("Показан замер в другом режиме — на процессоре или с другим языком. Своя оценка появится после нескольких диктовок.")}`;
    }
  }
  const memory = value.memory;
  if (memory.status === "loaded") {
    result.memory = t("Модель уже загружена. Для длинных записей может понадобиться дополнительная память.");
  } else if (memory.status === "gpu_unknown") {
    result.memory = t("Не удалось определить, хватит ли памяти видеокарты для этой модели.");
  } else {
    if (memory.status === "low") result.memory = t("Свободной памяти может не хватить. Закройте другие приложения или выберите модель поменьше.");
    else if (memory.status === "enough") result.memory = t("Памяти должно хватить для работы модели.");
    if (memory.required_bytes !== null) result.memory += ` ${t("Нужно примерно {p0} ГБ оперативной памяти.", { p0: decimal(memory.required_bytes / 1024 ** 3) })}`;
  }
  if (memory.available_bytes !== null && memory.status !== "gpu_unknown") {
    result.memory += ` ${t("Сейчас свободно: {p0} ГБ.", { p0: decimal(memory.available_bytes / 1024 ** 3) })}`;
  }
  return result;
}

export function meterPercent(score: number | null | undefined): number | null {
  return typeof score === "number" && Number.isFinite(score) ? Math.round(Math.max(0, Math.min(1, score)) * 100) : null;
}
