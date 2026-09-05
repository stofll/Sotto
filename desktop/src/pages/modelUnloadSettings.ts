/**
 * Через сколько простоя модель уходит из оперативной памяти.
 *
 * Значения продублированы в `src-tauri/src/config.rs`: настройку читают обе
 * стороны, и разойтись им нельзя — интерфейс показывал бы одно, а движок
 * выгружал бы по другому.
 */

/** Нет значения в конфиге — выгружаем через пять минут, а не «никогда». */
export const DEFAULT_MODEL_UNLOAD_MINUTES = 5;

/** `0` — не выгружать. */
export const MODEL_UNLOAD_CHOICES = [5, 10, 30, 0];

/** Сутки — это уже «никогда», просто записанное числом. */
const MAX_MODEL_UNLOAD_MINUTES = 24 * 60;

/**
 * Что на самом деле делает движок при таком значении конфига.
 *
 * Мусор и отрицательные числа откатываются к умолчанию, а не выключают
 * выгрузку: «не смогли прочитать» — это не «просили никогда». Порядок тот
 * же, что и в `config::model_unload_after_minutes`.
 */
export function modelUnloadMinutes(value: number | undefined | null): number {
  if (typeof value !== "number" || !Number.isInteger(value) || value < 0) {
    return DEFAULT_MODEL_UNLOAD_MINUTES;
  }
  return Math.min(value, MAX_MODEL_UNLOAD_MINUTES);
}

/**
 * Значения для списка в настройках.
 *
 * Конфиг правят и руками, и число оттуда может не совпасть ни с одним
 * пунктом. Тогда пункт добавляется, а не подменяется ближайшим: настройка
 * работает ровно так, как записана, и список обязан это показывать —
 * иначе первое же открытие настроек молча переписало бы её.
 */
export function modelUnloadOptions(current: number): number[] {
  const minutes = [...new Set([...MODEL_UNLOAD_CHOICES, current])].filter((value) => value > 0);
  minutes.sort((a, b) => a - b);
  // «Никогда» последним: это не самый долгий срок, а отказ от срока.
  return [...minutes, 0];
}
