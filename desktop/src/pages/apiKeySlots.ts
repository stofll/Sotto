import { PROVIDERS } from "./aiShared";
import type { ApiKeyInfo, ApiKeyStatus, ConfigResult } from "../bridge/types";
import { t } from "../i18n";

type LlmProfile = NonNullable<ConfigResult["ai_processing"]["profiles"]>[number];

export const EMPTY_KEY_INFO: ApiKeyInfo = { available: false, label: "", masked: "" };

export function profileKeyRef(profile: Pick<LlmProfile, "id" | "provider" | "api_key_ref">): string {
  return profile.api_key_ref || (profile.id === "default" ? profile.provider : `key_${profile.id}`);
}

export type Slot = {
  ref: string;
  kind: "profile" | "default" | "standalone";
  provider: string;
  title: string;
  sub: string;
  model?: string;
  info: ApiKeyInfo;
  isActive: boolean;
};

export function collectSlots(config: ConfigResult["ai_processing"] | null, apiKeys: ApiKeyStatus): Slot[] {
  const slots: Slot[] = [];
  const seen = new Set<string>();
  const activeId = config?.active_profile_id || config?.profile_id || null;

  const profiles = config?.profiles ?? [];
  for (const profile of profiles) {
    const ref = profileKeyRef(profile);
    if (seen.has(ref)) continue;
    seen.add(ref);
    const providerMeta = PROVIDERS.find((p) => p.id === profile.provider) ?? PROVIDERS[0];
    slots.push({
      ref,
      kind: "profile",
      provider: profile.provider,
      title: profile.name || providerMeta.name,
      sub: `${providerMeta.name} · ${profile.model || providerMeta.defaultModel}`,
      model: profile.model || providerMeta.defaultModel,
      info: apiKeys[ref] ?? EMPTY_KEY_INFO,
      isActive: profile.id === activeId,
    });
  }

  // Слоты без профиля. Без этого прохода ключ, созданный кнопкой «Добавить
  // ключ», не попадал в список ни разу: его ref (`key_<время>`) не совпадает
  // ни с ref профиля, ни с id провайдера.
  for (const slot of config?.key_slots ?? []) {
    if (!slot.ref || seen.has(slot.ref)) continue;
    seen.add(slot.ref);
    const providerMeta = PROVIDERS.find((p) => p.id === slot.provider) ?? PROVIDERS[0];
    const info = apiKeys[slot.ref] ?? EMPTY_KEY_INFO;
    slots.push({
      ref: slot.ref,
      kind: "standalone",
      provider: slot.provider,
      title: slot.label || info.label || providerMeta.name,
      sub: t("{p0} · не привязан к профилю", { p0: providerMeta.name }),
      info,
      isActive: false,
    });
  }

  for (const provider of PROVIDERS) {
    if (seen.has(provider.id)) continue;
    const info = apiKeys[provider.id] ?? EMPTY_KEY_INFO;
    if (!info.available) continue;
    seen.add(provider.id);
    slots.push({
      ref: provider.id,
      kind: "default",
      provider: provider.id,
      title: t("{p0} (общий)", { p0: provider.name }),
      sub: t("{p0} · общий слот провайдера", { p0: provider.name }),
      info,
      isActive: false,
    });
  }

  return slots;
}

