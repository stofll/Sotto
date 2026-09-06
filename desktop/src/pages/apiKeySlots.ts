// Both of these used to be declared here as well, character for character.
// One of the two copies is the one callers get, depending on which import they
// happened to write, and a change to either would have silently applied to
// half the app.
import { EMPTY_KEY_INFO, PROVIDERS, profileKeyRef } from "./aiShared";
import type { ApiKeyInfo, ApiKeyStatus, ConfigResult } from "../bridge/types";
import { t } from "../i18n";

export type KeySlotRecord = NonNullable<ConfigResult["ai_processing"]["key_slots"]>[number];

/**
 * Writes a ref down in the list of known key slots, if it is not there already.
 *
 * The OS store cannot be enumerated — `has_api_key` only answers about a ref
 * somebody already knows (see `key_slots` in bridge/types). A key created by
 * the wizard together with its profile was known through that profile alone,
 * so deleting the profile — which deliberately leaves the key in place — threw
 * the ref away: the key stayed in Credential Manager with nothing in the UI
 * pointing at it, impossible to see, reuse or delete. Every key the app creates
 * is registered here, and the record outlives the profile.
 */
export function withKeySlot(slots: KeySlotRecord[], record: KeySlotRecord): KeySlotRecord[] {
  return slots.some((item) => item.ref === record.ref) ? slots : [...slots, record];
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

  // Profile-less slots. Without this pass a key created by the «Добавить ключ»
  // button never made it into the list: its ref (`key_<timestamp>`) matches
  // neither a profile ref nor a provider id.
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

