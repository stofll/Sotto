import { describe, expect, it } from "vitest";
import { collectSlots } from "./apiKeySlots";
import type { ApiKeyStatus, ConfigResult } from "../bridge/types";

type Ai = ConfigResult["ai_processing"];

function ai(patch: Partial<Ai>): Ai {
  return {
    pipeline_mode: "hybrid",
    provider: "compatible",
    model: "gpt-4o-mini",
    prompt_preset: "plain",
    spend_limit_usd: 10,
    system_prompt: "",
    ...patch,
  } as Ai;
}

const filled = { available: true, label: "мой ключ", masked: "sk-123…7890" };

describe("collectSlots", () => {
  it("показывает слот без профиля, созданный кнопкой «Добавить ключ»", () => {
    // Ref такого слота (`key_<время>`) не совпадает ни с ref профиля, ни с id
    // провайдера — до появления прохода по key_slots строка не рисовалась
    // вообще, хотя ключ лежал в хранилище.
    const config = ai({ key_slots: [{ ref: "key_mq8j7rjc", label: "Cerebras", provider: "compatible" }] });
    const keys: ApiKeyStatus = { key_mq8j7rjc: filled };

    const slots = collectSlots(config, keys);

    expect(slots.map((s) => s.ref)).toContain("key_mq8j7rjc");
    const slot = slots.find((s) => s.ref === "key_mq8j7rjc")!;
    expect(slot.kind).toBe("standalone");
    expect(slot.title).toBe("Cerebras");
    expect(slot.info.masked).toBe("sk-123…7890");
  });

  it("рисует слот без профиля и когда ключа в нём ещё нет", () => {
    // Иначе удалить осиротевшую запись из интерфейса было бы нечем.
    const config = ai({ key_slots: [{ ref: "key_empty", label: "Пустой", provider: "openai" }] });

    const slots = collectSlots(config, {});

    const slot = slots.find((s) => s.ref === "key_empty");
    expect(slot).toBeDefined();
    expect(slot!.info.available).toBe(false);
  });

  it("не дублирует слот, если на тот же ref уже ссылается профиль", () => {
    const config = ai({
      profiles: [{ id: "profile_a", name: "Neura", provider: "compatible", model: "m", api_key_ref: "key_shared" }],
      key_slots: [{ ref: "key_shared", label: "тот же", provider: "compatible" }],
    });

    const slots = collectSlots(config, { key_shared: filled });

    expect(slots.filter((s) => s.ref === "key_shared")).toHaveLength(1);
    expect(slots.find((s) => s.ref === "key_shared")!.kind).toBe("profile");
  });

  it("не теряет слоты профилей и провайдеров из-за нового прохода", () => {
    const config = ai({
      profiles: [{ id: "profile_a", name: "Neura", provider: "compatible", model: "m", api_key_ref: "key_a" }],
      key_slots: [{ ref: "key_b", label: "отдельный", provider: "openai" }],
    });

    const slots = collectSlots(config, { key_a: filled, key_b: filled, openai: filled });

    expect(slots.map((s) => s.ref).sort()).toEqual(["key_a", "key_b", "openai"]);
  });
});
