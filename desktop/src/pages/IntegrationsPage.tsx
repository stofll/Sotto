import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "../bridge";
import { PageHeader, SectionLabel } from "../components/Shell";
import { Icon } from "../components/Icon";
import { CustomSelect, type SelectOption } from "../components/CustomSelect";
import { ProfileWizard, type ProfileWizardSeed } from "../components/ProfileWizard";
import type { ApiKeyStatus, ConfigResult } from "../bridge/types";
import { t, tPlural } from "../i18n";
import {
  activeConfigFromProfile,
  mergeAi,
  normalizeProfile,
  OPENCODE_GO_BASE_URL,
  ProviderMark,
  profileKeyRef,
  profilesForAi,
  PROVIDERS,
  PROVIDER_MODEL_OPTIONS,
  type AiConfig,
  type LlmProfile,
} from "./aiShared";
import { collectSlots, EMPTY_KEY_INFO, type Slot } from "./apiKeySlots";
import { ModelField, useProviderModels } from "./providerModels";

type Props = {
  config: AiConfig | null;
  apiKeys: ApiKeyStatus;
  onConfigChanged: (partial: Partial<ConfigResult>) => Promise<ConfigResult | null>;
  onApiKeysChanged: (next: ApiKeyStatus) => void;
};

type KeySlotRecord = NonNullable<ConfigResult["ai_processing"]["key_slots"]>[number];

type KeyFilterValue = "all" | "filled" | "empty";

type TestResponse = { available: boolean; message?: string; provider_error?: string; output?: string };

/** Ключ профиля в `testingKey` и `testResults`. Одна функция на все места,
 *  где он строится: строка, собранная по месту, молча разъезжается. */
function testKeyFor(profileId: string): string {
  return `profile:${profileId}`;
}

/** «Интеграции»: профили провайдеров сверху, слоты ключей снизу.
 *
 * Раньше это были две страницы. Каталог из 15 провайдеров и сетка
 * OpenAI-compatible пресетов, стоявшие сверху «Провайдеров», удалены целиком:
 * и то и другое дублировало первый шаг мастера «Новый профиль», где выбор
 * стоит в контексте, а на странице просто занимал первый экран. Вместе с
 * каталогом ушли редактор дефолтной модели провайдера (значение по-прежнему
 * читается из конфига как подсказка мастеру) и тест на уровне провайдера —
 * тест профиля честнее, он проверяет реальную связку ключ + модель + Base
 * URL. */
export function IntegrationsPage({ config: ai, apiKeys, onConfigChanged, onApiKeysChanged }: Props) {
  const profiles = useMemo(() => profilesForAi(ai), [ai]);
  const providerModels = useMemo(() => ai?.provider_models ?? {}, [ai?.provider_models]);

  const [draftProfiles, setDraftProfiles] = useState<LlmProfile[]>(profiles);
  const [expandedProfiles, setExpandedProfiles] = useState<Set<string>>(() => new Set());
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [testingKey, setTestingKey] = useState<string | null>(null);
  const [testResults, setTestResults] = useState<Record<string, { ok: boolean; text: string }>>({});
  const [wizardSeed, setWizardSeed] = useState<ProfileWizardSeed | null>(null);
  const remoteModels = useProviderModels();

  // ── Ключи ──────────────────────────────────────────────────────────────
  const [adding, setAdding] = useState(false);
  const [newProvider, setNewProvider] = useState<string>(PROVIDERS[0].id);
  const [newLabel, setNewLabel] = useState("");
  const [newKey, setNewKey] = useState("");
  const [newRevealed, setNewRevealed] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  const [replaceLabel, setReplaceLabel] = useState("");
  const [replaceKey, setReplaceKey] = useState("");
  const [replaceRevealed, setReplaceRevealed] = useState(false);
  const [search, setSearch] = useState("");
  const [keyFilter, setKeyFilter] = useState<KeyFilterValue>("all");
  const [providerFilter, setProviderFilter] = useState<string>("all");
  const keysRef = useRef<HTMLDivElement>(null);

  const slots = useMemo(() => collectSlots(ai, apiKeys), [ai, apiKeys]);

  const filteredSlots = useMemo(() => {
    const q = search.trim().toLowerCase();
    return slots.filter((slot) => {
      if (keyFilter === "filled" && !slot.info.available) return false;
      if (keyFilter === "empty" && slot.info.available) return false;
      if (providerFilter !== "all" && slot.provider !== providerFilter) return false;
      if (!q) return true;
      const hay = `${slot.title} ${slot.sub} ${slot.ref} ${slot.info.label} ${slot.info.masked}`.toLowerCase();
      return hay.includes(q);
    });
  }, [slots, search, keyFilter, providerFilter]);

  useEffect(() => setDraftProfiles(profiles), [JSON.stringify(profiles)]);

  function showMessage(text: string) {
    setMessage(text);
    window.setTimeout(() => setMessage((current) => (current === text ? null : current)), 2500);
  }

  async function saveAi(nextAi: AiConfig, text: string) {
    setSaving(true);
    try {
      await onConfigChanged({ ai_processing: nextAi });
      showMessage(text);
    } finally {
      setSaving(false);
    }
  }

  function updateProfile(profileId: string, patch: Partial<LlmProfile>) {
    setDraftProfiles((current) =>
      current.map((profile) => {
        if (profile.id !== profileId) return profile;
        const nextProvider = patch.provider ?? profile.provider;
        const provider = PROVIDERS.find((item) => item.id === nextProvider) ?? PROVIDERS[0];
        const providerChanged = patch.provider && patch.provider !== profile.provider;
        return {
          ...profile,
          ...patch,
          model: providerChanged
            ? providerModels[nextProvider] || provider.defaultModel
            : (patch.model ?? profile.model),
          api_key_ref: providerChanged ? nextProvider : (patch.api_key_ref ?? profile.api_key_ref),
          base_url:
            providerChanged && nextProvider === "opencode-go"
              ? OPENCODE_GO_BASE_URL
              : (patch.base_url ?? profile.base_url ?? ""),
        };
      }),
    );
    // Смена провайдера обесценивает подтянутый список: он был про другой
    // API. Тянем новый сразу — это явное действие пользователя, а не
    // фоновый опрос, и без него в подсказках остались бы чужие модели.
    if (patch.provider) {
      const profile = draftProfiles.find((item) => item.id === profileId);
      const nextBaseUrl = patch.provider === "opencode-go" ? OPENCODE_GO_BASE_URL : profile?.base_url;
      void remoteModels.load(profileId, {
        provider: patch.provider,
        baseUrl: nextBaseUrl || undefined,
        apiKeyRef: patch.provider,
      });
    }
  }

  async function saveProfile(profileId: string) {
    if (!ai) return;
    const profile = draftProfiles.find((item) => item.id === profileId);
    if (!profile || !profile.model.trim()) return;
    const normalized = { ...profile, model: profile.model.trim(), base_url: profile.base_url?.trim() || "" };
    const nextProfiles = draftProfiles.map((item) => (item.id === profileId ? normalized : item));
    const activeId = ai.active_profile_id || ai.profile_id || nextProfiles[0]?.id;
    const activeProfile = nextProfiles.find((item) => item.id === activeId) ?? normalized;
    await saveAi(activeConfigFromProfile(ai, activeProfile, nextProfiles), t("Профиль провайдера сохранён."));
  }

  async function setActiveProfile(profile: LlmProfile) {
    if (!ai) return;
    const target = profiles.find((item) => item.id === profile.id) ?? profile;
    await saveAi(activeConfigFromProfile(ai, target, profiles), t("Профиль «{p0}» теперь активный.", { p0: target.name }));
  }

  function resetProfile(profileId: string) {
    const original = profiles.find((item) => item.id === profileId);
    if (!original) return;
    setDraftProfiles((current) => current.map((item) => (item.id === profileId ? original : item)));
  }

  function startRename(profile: LlmProfile) {
    setRenamingId(profile.id);
    setRenameDraft(profile.name);
  }

  /// Переименование сохраняется сразу, а не через общую кнопку «Сохранить»:
  /// карандаш стоит в свёрнутой строке, а кнопка живёт в раскрытом
  /// редакторе — до неё пользователь бы просто не доехал. За основу берётся
  /// сохранённый профиль, а не черновик, чтобы вместе с именем не записать
  /// правки, которых пользователь ещё не подтверждал.
  async function commitRename(profile: LlmProfile) {
    const next = renameDraft.trim();
    setRenamingId(null);
    if (!ai || !next || next === profile.name) return;
    const nextProfiles = profiles.map((item) => (item.id === profile.id ? { ...item, name: next } : item));
    const activeId = ai.active_profile_id || ai.profile_id || nextProfiles[0]?.id;
    const activeProfile = nextProfiles.find((item) => item.id === activeId) ?? nextProfiles[0];
    await saveAi(activeConfigFromProfile(ai, activeProfile, nextProfiles), t("Профиль переименован."));
  }

  /// Дубликат наследует ключ оригинала, а не пустой слот: копию делают ради
  /// другой модели или промпта, и требовать заново вводить тот же ключ было
  /// лишним шагом.
  async function duplicateProfile(source: LlmProfile) {
    if (!ai) return;
    const id = `profile_${Date.now().toString(36)}`;
    const copy = normalizeProfile(ai, {
      ...source,
      id,
      name: t("{p0} копия", { p0: source.name }),
      api_key_ref: profileKeyRef(source),
    });
    const nextProfiles = [...profiles, copy];
    await saveAi(activeConfigFromProfile(ai, copy, nextProfiles), t("Профиль продублирован."));
    setExpandedProfiles((prev) => new Set([...prev, id]));
  }

  async function deleteProfile(target: LlmProfile) {
    if (!ai) return;
    if (!window.confirm(t("Удалить профиль «{p0}»? API-ключ не удаляется автоматически.", { p0: target.name }))) return;
    const nextProfiles = profiles.filter((profile) => profile.id !== target.id);
    if (nextProfiles.length === 0) {
      // Удалён последний профиль: плоские поля LLM остаются рабочими, гаснет
      // только ссылка на активный — то же состояние, что у свежей установки.
      await saveAi(mergeAi(ai, { profiles: [], active_profile_id: "", profile_id: "" }), t("Профиль удалён."));
      return;
    }
    const nextActiveId = ai.active_profile_id === target.id
      ? nextProfiles[0].id
      : (ai.active_profile_id || nextProfiles[0].id);
    const nextActive = nextProfiles.find((profile) => profile.id === nextActiveId) ?? nextProfiles[0];
    await saveAi(activeConfigFromProfile(ai, nextActive, nextProfiles), t("Профиль удалён."));
  }

  function toggleProfile(id: string) {
    setExpandedProfiles((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  async function runTest(key: string, payload: Record<string, unknown>) {
    setTestingKey(key);
    setTestResults((prev) => { const next = { ...prev }; delete next[key]; return next; });
    try {
      const result = await invoke<TestResponse>("test_ai_prompt", payload);
      const text = result.available
        ? t("Тест пройден — LLM ответила.")
        : (result.message || result.provider_error || t("Тест не прошёл."));
      setTestResults((prev) => ({ ...prev, [key]: { ok: result.available, text } }));
    } catch (e) {
      setTestResults((prev) => ({ ...prev, [key]: { ok: false, text: e instanceof Error ? e.message : String(e) } }));
    } finally {
      setTestingKey((current) => (current === key ? null : current));
    }
  }

  async function testProfile(profile: LlmProfile) {
    if (!ai) return;
    const testKey = testKeyFor(profile.id);
    const model = profile.model?.trim();
    if (!model) {
      setTestResults((prev) => ({ ...prev, [testKey]: { ok: false, text: t("Укажите модель перед тестом.") } }));
      return;
    }
    const keyRef = profileKeyRef(profile);
    if (!apiKeys[keyRef]?.available) {
      setTestResults((prev) => ({ ...prev, [testKey]: { ok: false, text: t("Slot не содержит ключ. Сохраните ключ или выберите другой slot.") } }));
      return;
    }
    await runTest(testKey, {
      profile_id: profile.id,
      profile_name: profile.name,
      provider: profile.provider,
      model,
      api_key_ref: keyRef,
      base_url: profile.base_url ?? "",
      system_prompt: profile.system_prompt ?? ai.system_prompt ?? "",
    });
  }

  async function createProfileFromWizard(payload: { profile: LlmProfile; newKey?: { ref: string; value: string; label: string } }) {
    if (!ai) return;
    if (payload.newKey) {
      const result = await invoke<{ saved: boolean; label: string; masked: string }>("save_api_key", {
        key_id: payload.newKey.ref,
        key: payload.newKey.value,
        label: payload.newKey.label,
      });
      if (!result.saved) throw new Error(t("Не удалось сохранить ключ."));
      onApiKeysChanged({ ...apiKeys, [payload.newKey.ref]: { available: true, label: result.label, masked: result.masked } });
    }
    const nextProfiles = [...profiles, payload.profile];
    await saveAi(
      activeConfigFromProfile(ai, payload.profile, nextProfiles),
      t("Профиль «{p0}» создан.", { p0: payload.profile.name }),
    );
    setExpandedProfiles((prev) => new Set([...prev, payload.profile.id]));
  }

  // ── Ключи: сохранение, удаление, добавление ────────────────────────────

  /// Записывает список слотов без профиля в конфиг. Метка хранится здесь,
  /// а не только в хранилище ОС: портимого поля для неё есть лишь у Windows,
  /// на macOS и Linux она бы терялась при перезапуске.
  async function saveKeySlots(next: KeySlotRecord[]) {
    if (!ai) return;
    await onConfigChanged({ ai_processing: { ...ai, key_slots: next } });
  }

  async function saveSlot(slot: Slot, label: string, key: string) {
    const trimmed = key.trim();
    if (!trimmed) {
      showMessage(t("Введите значение ключа."));
      return;
    }
    const result = await invoke<{ saved: boolean; label: string; masked: string }>("save_api_key", {
      key_id: slot.ref,
      key: trimmed,
      label: label.trim(),
    });
    if (!result.saved) {
      showMessage(t("Не удалось сохранить ключ."));
      return;
    }
    onApiKeysChanged({ ...apiKeys, [slot.ref]: { available: true, label: result.label, masked: result.masked } });
    if (slot.kind === "standalone") {
      await saveKeySlots(
        (ai?.key_slots ?? []).map((item) => (item.ref === slot.ref ? { ...item, label: result.label } : item)),
      );
    }
    showMessage(t("Ключ сохранён."));
    cancelEdit();
  }

  async function deleteSlot(slot: Slot) {
    if (!window.confirm(t("Удалить ключ «{p0}»? Профили, ссылающиеся на этот слот, останутся без ключа.", { p0: slot.title }))) return;
    await invoke<{ deleted: boolean }>("delete_api_key", { key_id: slot.ref });
    const next = { ...apiKeys };
    next[slot.ref] = EMPTY_KEY_INFO;
    onApiKeysChanged(next);
    // Слот без профиля существует только записью в конфиге — убираем и её,
    // иначе строка останется висеть навсегда пустой.
    if (slot.kind === "standalone") {
      await saveKeySlots((ai?.key_slots ?? []).filter((item) => item.ref !== slot.ref));
    }
    showMessage(t("Ключ удалён."));
  }

  async function addCustomKey() {
    const trimmed = newKey.trim();
    if (!trimmed) {
      showMessage(t("Введите значение ключа."));
      return;
    }
    const ref = `key_${Date.now().toString(36)}`;
    const label = newLabel.trim() || PROVIDERS.find((p) => p.id === newProvider)?.name || "";
    const result = await invoke<{ saved: boolean; label: string; masked: string }>("save_api_key", {
      key_id: ref,
      key: trimmed,
      label,
    });
    if (!result.saved) {
      showMessage(t("Не удалось сохранить ключ."));
      return;
    }
    onApiKeysChanged({ ...apiKeys, [ref]: { available: true, label: result.label, masked: result.masked } });
    await saveKeySlots([...(ai?.key_slots ?? []), { ref, label, provider: newProvider }]);
    setNewKey("");
    setNewLabel("");
    setNewRevealed(false);
    setAdding(false);
    showMessage(t("Ключ сохранён. Привяжите его к профилю в поле «Key ref»."));
  }

  function startEdit(slot: Slot) {
    setEditing(slot.ref);
    setReplaceLabel(slot.info.label);
    setReplaceKey("");
    setReplaceRevealed(false);
  }

  function cancelEdit() {
    setEditing(null);
    setReplaceKey("");
    setReplaceLabel("");
    setReplaceRevealed(false);
  }

  /// «Задать ключ» из строки профиля. Раньше это была навигация на соседнюю
  /// вкладку; теперь ключи на той же странице, поэтому открываем редактор
  /// нужного слота и подводим к нему экран. Слота может ещё не быть —
  /// тогда открывается модалка добавления с уже выбранным провайдером.
  function focusKeyForProfile(profile: LlmProfile) {
    const ref = profileKeyRef(profile);
    const slot = slots.find((item) => item.ref === ref);
    if (slot) {
      startEdit(slot);
      keysRef.current?.scrollIntoView({ behavior: "smooth", block: "start" });
      return;
    }
    setNewProvider(profile.provider);
    setAdding(true);
  }

  const keyOptions = Object.entries(apiKeys).filter(([, info]) => info.available);
  const activeProfileId = ai?.active_profile_id || ai?.profile_id || null;
  const profileOriginalById = new Map(profiles.map((profile) => [profile.id, profile]));
  const hasChanges = JSON.stringify(draftProfiles) !== JSON.stringify(profiles);
  const missingProfileKeys = draftProfiles.filter((profile) => !apiKeys[profileKeyRef(profile)]?.available).length;

  return (
    <div className="page">
      <PageHeader
        title={t("Провайдеры и ключи")}
        actions={
          <>
            <span className={hasChanges ? "pill warn" : "pill ok dot"}>
              {hasChanges ? t("Есть изменения") : t("Синхронизировано")}
            </span>
            {/* Ключ и профиль — два способа начать работу на этой странице, и
                оба живут в шапке. Кнопка «Добавить ключ» стояла в строке
                фильтров списка ключей: чтобы добавить первый ключ, нужно было
                сначала доскроллить до списка, которого ещё нет. */}
            <button className="btn btn--ghost" onClick={() => setAdding(true)}>
              <Icon name="plus" size={12}/>  {t("Добавить ключ")} </button>
            <button className="btn btn--primary" onClick={() => setWizardSeed({})}>
              <Icon name="plus" size={12}/>  {t("Новый профиль")} </button>
          </>
        }
      />

      {message && (
        <div role="status" style={{ padding: "10px 12px", borderRadius: 8, background: "var(--surface-2)", border: "1px solid var(--border)", font: "500 12px/1.4 var(--font-sans)", marginBottom: 12 }}>
          {message}
        </div>
      )}
      {missingProfileKeys > 0 && (
        <div role="alert" style={{ padding: "10px 12px", borderRadius: 8, background: "var(--accent-soft)", border: "1px solid var(--accent-line)", color: "var(--ink)", font: "500 12px/1.4 var(--font-sans)", display: "flex", gap: 8, alignItems: "center", marginBottom: 12 }}>
          <Icon name="info" size={12}/> {missingProfileKeys}  {t("проф. ссылаются на отсутствующий API-ключ. Добавьте ключ или выберите другой slot.")} </div>
      )}

      {/* ── Профили провайдеров ─────────────────────────────────────────── */}
      <div className="flex-row" style={{ justifyContent: "space-between", alignItems: "center", marginBottom: 8 }}>
        <SectionLabel>{t("Профили провайдеров")}</SectionLabel>
        <span style={{ font: "500 11px/1 var(--font-mono)", color: "var(--ink-mute)" }}>
          {draftProfiles.length} {tPlural(draftProfiles.length, ["профиль", "профиля", "профилей"])}
        </span>
      </div>

      <section className="card prov-list-card">
        {draftProfiles.length === 0 && (
          <div style={{ padding: 16, color: "var(--ink-mute)", font: "400 12px/1.5 var(--font-sans)" }}>
            {t("Профилей ещё нет. Профиль — это связка «провайдер + ключ + модель»; из него LLM-обработка берёт всё, что ей нужно.")}
          </div>
        )}
        {draftProfiles.map((profile, i) => {
          const provider = PROVIDERS.find((item) => item.id === profile.provider) ?? PROVIDERS[0];
          const keyRef = profileKeyRef(profile);
          const keyInfo = apiKeys[keyRef];
          const suggestions = PROVIDER_MODEL_OPTIONS[profile.provider] ?? [];
          const original = profileOriginalById.get(profile.id);
          const profileDirty = JSON.stringify(profile) !== JSON.stringify(original);
          const isOpen = expandedProfiles.has(profile.id);
          const isActive = profile.id === activeProfileId;
          const isRenaming = renamingId === profile.id;
          const maskedKey = keyInfo?.available ? (keyInfo.masked || "—") : "";
          const testKey = testKeyFor(profile.id);
          const testState = testResults[testKey];
          const testRunning = testingKey === testKey;

          return (
            <div key={profile.id} style={{ borderTop: i === 0 ? "none" : "1px solid var(--line-soft)" }}>
              {/* Строка — не одна большая кнопка: внутрь встают карандаш и
                  поле переименования, а кнопка в кнопке невалидна. */}
              <div className="prov-row prov-row--split">
                <button type="button" className="prov-row__main" onClick={() => toggleProfile(profile.id)} aria-expanded={isOpen}>
                  <span className="prov-chev" data-open={isOpen ? "true" : "false"}>
                    <Icon name="chev-right" size={14}/>
                  </span>
                  {isActive
                    ? <span className="prov-profile-dot"/>
                    : <ProviderMark provider={provider} size={14}/>}
                  <div className="prov-id">
                    <div className="prov-id__top">
                      {!isRenaming && <span className="prov-id__name">{profile.name}</span>}
                      {isActive && <span className="pill accent">{t("активный")}</span>}
                      {profileDirty && <span className="pill accent">{t("изменено")}</span>}
                    </div>
                    <span className="prov-id__sub mono">
                      {provider.name} · {profile.model}{maskedKey ? ` · ${maskedKey}` : ""}
                    </span>
                  </div>
                </button>

                {isRenaming && (
                  <input
                    className="field"
                    autoFocus
                    value={renameDraft}
                    onChange={(e) => setRenameDraft(e.target.value)}
                    onBlur={() => void commitRename(profile)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") (e.target as HTMLInputElement).blur();
                      else if (e.key === "Escape") setRenamingId(null);
                    }}
                    maxLength={64}
                    style={{ height: 28, flex: "1 1 200px", minWidth: 0, fontSize: 13 }}
                    aria-label={t("Название профиля")}
                  />
                )}

                <div className="prov-row__tools">
                  <button
                    type="button"
                    className="icon-btn"
                    onClick={() => startRename(profile)}
                    disabled={isRenaming}
                    title={t("Переименовать профиль")}
                    aria-label={t("Переименовать профиль")}
                  >
                    <Icon name="pencil" size={12}/>
                  </button>
                  <button
                    type="button"
                    className="icon-btn"
                    onClick={() => void duplicateProfile(profile)}
                    title={t("Дублировать профиль")}
                    aria-label={t("Дублировать профиль")}
                  >
                    <Icon name="copy" size={12}/>
                  </button>
                  <button
                    type="button"
                    className="icon-btn icon-btn--danger"
                    onClick={() => void deleteProfile(profile)}
                    title={t("Удалить профиль")}
                    aria-label={t("Удалить профиль")}
                  >
                    <Icon name="trash" size={12}/>
                  </button>
                </div>

                <div className="prov-meta">
                  <span className={keyInfo?.available ? "pill ok dot" : "pill warn"}>
                    {keyInfo?.available ? t("ключ есть") : t("нет ключа")}
                  </span>
                </div>
              </div>

              {isOpen && (
                <div className="prov-exp">
                  <div className="prov-exp-grid">
                    <div className="set-cell">
                      <span className="set-label">Provider</span>
                      <select
                        className="field"
                        value={profile.provider}
                        onChange={(e) => updateProfile(profile.id, { provider: e.target.value })}
                        style={{ height: 30 }}
                      >
                        {PROVIDERS.map((item) => (
                          <option key={item.id} value={item.id}>{item.name}</option>
                        ))}
                      </select>
                    </div>
                    <div className="set-cell">
                      <span className="set-label">Model</span>
                      {/* Ключ кэша — профиль, а не провайдер: у каждого
                          профиля свои base_url и ключ, значит и список
                          моделей свой. */}
                      <ModelField
                        cacheKey={profile.id}
                        value={profile.model}
                        onChange={(next) => updateProfile(profile.id, { model: next })}
                        fallbackSuggestions={suggestions}
                        query={{
                          provider: profile.provider,
                          baseUrl: profile.base_url ?? undefined,
                          apiKeyRef: keyRef,
                        }}
                        state={remoteModels}
                      />
                    </div>
                    <div className="set-cell">
                      <span className="set-label">Key ref</span>
                      <select
                        className="field"
                        value={keyRef}
                        onChange={(e) => updateProfile(profile.id, { api_key_ref: e.target.value })}
                        style={{ height: 30 }}
                      >
                        <option value={profile.provider}>{profile.provider}</option>
                        {keyOptions.map(([ref, info]) => (
                          <option key={ref} value={ref}>{info.label || ref} · {info.masked}</option>
                        ))}
                      </select>
                    </div>
                  </div>

                  {(profile.provider === "compatible" || profile.provider === "opencode-go" || profile.base_url) && (
                    <div className="set-cell">
                      <span className="set-label">Base URL</span>
                      <input
                        className="field mono"
                        value={profile.base_url ?? ""}
                        onChange={(e) => updateProfile(profile.id, { base_url: e.target.value })}
                        placeholder={profile.provider === "opencode-go" ? OPENCODE_GO_BASE_URL : "https://api.example.com/v1"}
                        style={{ height: 30, fontSize: 12 }}
                      />
                    </div>
                  )}

                  {!keyInfo?.available && (
                    <div style={{ padding: "8px 10px", borderRadius: 8, background: "var(--accent-soft)", border: "1px solid var(--accent-line)", color: "var(--ink)", font: "500 11px/1.4 var(--font-sans)", display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
                      <span>Slot <span className="mono">{keyRef}</span> {t("пуст.")}</span>
                      <button className="btn btn--ghost" style={{ height: 24 }} onClick={() => focusKeyForProfile(profile)}>
                        <Icon name="key" size={11}/>  {t("Задать ключ")} </button>
                    </div>
                  )}

                  <div className="prov-exp__actions">
                    <button
                      className={isActive ? "btn btn--primary" : "btn btn--ghost"}
                      onClick={() => { if (!isActive) void setActiveProfile(profile); }}
                      disabled={isActive || saving}
                      title={t("Сделать этот профиль активным для pipeline")}
                    >
                      <Icon name={isActive ? "check" : "spark"} size={12}/> {isActive ? t("Активный профиль") : t("Сделать активным")}
                    </button>
                    <div className="grow"/>
                    <button className="btn btn--ghost" onClick={() => resetProfile(profile.id)} disabled={saving || !profileDirty}>{t("Отмена")}</button>
                    <button className="btn btn--primary" onClick={() => void saveProfile(profile.id)} disabled={saving || !profileDirty || !profile.model.trim()}>
                      <Icon name="check" size={12}/>  {t("Сохранить")} </button>
                    <div className="grow"/>
                    <button
                      className="btn btn--ghost"
                      onClick={() => void testProfile(profile)}
                      disabled={testRunning || !profile.model.trim()}
                      title={t("Короткий запрос к провайдеру: жив ли ключ и отвечает ли эндпоинт")}
                    >
                      <Icon name="test" size={12}/>
                      {testRunning ? t("Проверяю…") : t("Проверить связь")}
                    </button>
                  </div>
                  {testState && (
                    <div
                      role="status"
                      style={{
                        marginTop: 8,
                        padding: "8px 10px",
                        borderRadius: 8,
                        font: "500 12px/1.4 var(--font-sans)",
                        background: testState.ok ? "var(--accent-soft)" : "var(--surface-2)",
                        border: `1px solid ${testState.ok ? "var(--accent-line)" : "var(--border)"}`,
                        color: testState.ok ? "var(--ink)" : "var(--err)",
                      }}
                    >
                      <Icon name={testState.ok ? "check" : "info"} size={12}/>{" "}
                      {testState.text}
                    </div>
                  )}
                </div>
              )}
            </div>
          );
        })}
      </section>

      {/* ── API-ключи ───────────────────────────────────────────────────── */}
      <div ref={keysRef} className="integrations-split">
        <div className="flex-row" style={{ justifyContent: "space-between", alignItems: "center", gap: 10, flexWrap: "wrap", marginBottom: 8 }}>
          <SectionLabel>{t("API-ключи")}</SectionLabel>
          <div className="flex-row" style={{ gap: 8, flexWrap: "wrap", alignItems: "center" }}>
            <label className="input-search" style={{ flex: "1 1 180px", maxWidth: 260 }}>
              <span className="input-search__icon"><Icon name="search" size={13}/></span>
              <input
                className="field"
                type="text"
                placeholder={t("Поиск по имени / slot / маске…")}
                value={search}
                onChange={(e) => setSearch(e.target.value)}
              />
              {search && (
                <button type="button" className="btn btn--ghost" onClick={() => setSearch("")} style={{ height: 22, padding: "0 6px", position: "absolute", right: 6, top: "50%", transform: "translateY(-50%)" }} aria-label={t("Очистить поиск")}>
                  <Icon name="x" size={11}/>
                </button>
              )}
            </label>
            <div style={{ width: 180 }}>
              <CustomSelect<string>
                value={providerFilter}
                options={[
                  { value: "all", label: t("Все провайдеры") },
                  ...PROVIDERS.map<SelectOption<string>>((p) => ({ value: p.id, label: p.name })),
                ]}
                onChange={(next) => setProviderFilter(next)}
              />
            </div>
            <div style={{ width: 150 }}>
              <CustomSelect<KeyFilterValue>
                value={keyFilter}
                options={[
                  { value: "all", label: t("Любые слоты") },
                  { value: "filled", label: t("С ключом") },
                  { value: "empty", label: t("Без ключа") },
                ]}
                onChange={(next) => setKeyFilter(next)}
              />
            </div>
          </div>
        </div>

        <section className="card keys-card">
          {slots.length === 0 && (
            <div className="keys-empty">
               {t("Сохранённых ключей нет. Добавьте первый ключ — он появится в этом списке и сможет быть привязан к любому профилю.")} </div>
          )}
          {slots.length > 0 && filteredSlots.length === 0 && (
            <div className="keys-empty">
               {t("Под выбранный фильтр ничего не подошло.")} </div>
          )}
          {filteredSlots.map((slot) => {
            const provider = PROVIDERS.find((p) => p.id === slot.provider) ?? PROVIDERS[0];
            const isEditing = editing === slot.ref;
            return (
              <div key={slot.ref} className="keys-row" title={`slot: ${slot.ref}`}>
                <span className="keys-row__dot" data-active={slot.isActive ? "true" : "false"} title={slot.isActive ? t("Используется активным профилем") : undefined}/>
                <ProviderMark provider={provider} size={16}/>
                <div className="keys-row__id">
                  <div className="keys-row__title">{slot.title}</div>
                  <div className="keys-row__sub">{slot.sub}</div>
                </div>
                <div className="keys-row__chips">
                  {slot.info.available
                    ? <span className="pill ok dot mono">{slot.info.masked || "•••"}</span>
                    : <span className="pill warn">{t("не задан")}</span>}
                  {slot.isActive && <span className="pill accent">{t("активный")}</span>}
                </div>
                <div className="keys-row__actions">
                  <button className="icon-btn" onClick={() => startEdit(slot)} title={slot.info.available ? t("Заменить ключ") : t("Задать ключ")} aria-label={slot.info.available ? t("Заменить ключ") : t("Задать ключ")}>
                    <Icon name="pencil" size={12}/>
                  </button>
                  {slot.info.available && (
                    <button className="icon-btn icon-btn--danger" onClick={() => void deleteSlot(slot)} title={t("Удалить ключ")} aria-label={t("Удалить ключ")}>
                      <Icon name="trash" size={12}/>
                    </button>
                  )}
                </div>
                {isEditing && (
                  <div className="keys-row__edit">
                    <input
                      className="field"
                      value={replaceLabel}
                      onChange={(e) => setReplaceLabel(e.target.value)}
                      placeholder={t("Метка ключа (опционально)")}
                      maxLength={64}
                      style={{ height: 30 }}
                    />
                    <div className="keys-row__edit-row">
                      <input
                        className="field mono"
                        type={replaceRevealed ? "text" : "password"}
                        value={replaceKey}
                        onChange={(e) => setReplaceKey(e.target.value)}
                        placeholder={t("Новое значение ключа")}
                        style={{ flex: 1, fontSize: 12 }}
                        autoFocus
                      />
                      <button
                        type="button"
                        className="btn btn--ghost"
                        onClick={() => setReplaceRevealed((v) => !v)}
                        aria-label={replaceRevealed ? t("Скрыть ключ") : t("Показать ключ")}
                        title={replaceRevealed ? t("Скрыть") : t("Показать")}
                        style={{ height: 30 }}
                      >
                        <Icon name={replaceRevealed ? "eye-off" : "eye"} size={13}/>
                      </button>
                      <button className="btn btn--primary" onClick={() => void saveSlot(slot, replaceLabel, replaceKey)} disabled={!replaceKey.trim()} style={{ height: 30 }}>
                        <Icon name="check" size={12}/>  {t("Сохранить")} </button>
                      <button className="btn btn--ghost" onClick={cancelEdit} style={{ height: 30 }}>{t("Отмена")}</button>
                    </div>
                  </div>
                )}
              </div>
            );
          })}
        </section>
      </div>

      {wizardSeed && (
        <ProfileWizard
          apiKeys={apiKeys}
          existingProfiles={draftProfiles}
          seed={wizardSeed}
          onClose={() => setWizardSeed(null)}
          onCreate={createProfileFromWizard}
        />
      )}

      {adding && (
        <div className="modal-overlay" onMouseDown={(e) => { if (e.target === e.currentTarget) { setAdding(false); setNewRevealed(false); } }}>
          <div className="modal" role="dialog" aria-modal="true">
            <div className="modal__head">
              <div>
                <h2>{t("Новый API-ключ")}</h2>
                <div className="sub">{t("Сохраняется в DPAPI, отдельным слотом. Привязать к профилю можно потом.")}</div>
              </div>
              <button className="modal__close" onClick={() => { setAdding(false); setNewRevealed(false); }} aria-label={t("Закрыть")}><Icon name="x" size={14}/></button>
            </div>
            <div className="modal__body">
              <label style={{ display: "grid", gap: 6 }}>
                <span className="wizard-label">{t("Провайдер")}</span>
                <select className="field" value={newProvider} onChange={(e) => setNewProvider(e.target.value)} style={{ height: 36 }}>
                  {PROVIDERS.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
                </select>
              </label>
              <label style={{ display: "grid", gap: 6 }}>
                <span className="wizard-label">{t("Метка (опционально)")}</span>
                <input className="field" value={newLabel} onChange={(e) => setNewLabel(e.target.value)} placeholder={t("например \"Личный Cerebras\"")} maxLength={64}/>
              </label>
              <label style={{ display: "grid", gap: 6 }}>
                <span className="wizard-label">{t("Ключ")}</span>
                <div style={{ display: "flex", gap: 6 }}>
                  <input
                    className="field mono"
                    type={newRevealed ? "text" : "password"}
                    value={newKey}
                    onChange={(e) => setNewKey(e.target.value)}
                    placeholder="sk-..."
                    style={{ flex: 1 }}
                  />
                  <button
                    type="button"
                    className="btn btn--ghost"
                    onClick={() => setNewRevealed((v) => !v)}
                    aria-label={newRevealed ? t("Скрыть ключ") : t("Показать ключ")}
                    title={newRevealed ? t("Скрыть") : t("Показать")}
                  >
                    <Icon name={newRevealed ? "eye-off" : "eye"} size={13}/>
                  </button>
                </div>
              </label>
            </div>
            <div className="modal__foot">
              <button className="btn btn--primary" onClick={() => void addCustomKey()} disabled={!newKey.trim()}>
                <Icon name="check" size={12}/>  {t("Сохранить ключ")} </button>
              <button className="btn btn--ghost" onClick={() => { setAdding(false); setNewRevealed(false); }}>{t("Отмена")}</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
