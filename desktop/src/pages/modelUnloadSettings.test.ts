import { describe, expect, it } from "vitest";
import {
  DEFAULT_MODEL_UNLOAD_MINUTES,
  modelUnloadMinutes,
  modelUnloadOptions,
} from "./modelUnloadSettings";

describe("выгрузка модели по простою", () => {
  it("без значения в конфиге выгружает, а не держит вечно", () => {
    expect(modelUnloadMinutes(undefined)).toBe(DEFAULT_MODEL_UNLOAD_MINUTES);
  });

  it("ноль — это «никогда», а не пропущенное значение", () => {
    expect(modelUnloadMinutes(0)).toBe(0);
  });

  it("нечитаемое значение откатывает к умолчанию, а не выключает выгрузку", () => {
    for (const value of [-5, 5.5, Number.NaN, null]) {
      expect(modelUnloadMinutes(value as number)).toBe(DEFAULT_MODEL_UNLOAD_MINUTES);
    }
  });

  it("срок длиннее суток показывается тем, чем стал, — сутками", () => {
    expect(modelUnloadMinutes(100_000)).toBe(24 * 60);
  });

  it("«никогда» стоит последним, а не среди сроков", () => {
    expect(modelUnloadOptions(5)).toEqual([5, 10, 30, 0]);
  });

  it("значение из-под руки попадает в список, а не подменяется ближайшим", () => {
    expect(modelUnloadOptions(2)).toEqual([2, 5, 10, 30, 0]);
  });
});
