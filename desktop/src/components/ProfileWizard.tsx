import { useMemo, useState } from "react";
import { Icon } from "./Icon";
import { CustomSelect, type SelectOption } from "./CustomSelect";
import { Hint } from "./Hint";
import {
  CATALOG_GROUPS,
  LogoMark,
  MODEL_HINTS,
  OPENCODE_GO_BASE_URL,
  PROVIDERS,
  PROVIDER_CATALOG,
  PROVIDER_MODEL_OPTIONS,
  type CompatiblePreset,
  type LlmProfile,
} from "../pages/aiShared";
import { apiKeyBlocks, checkApiKey, keyIsOptional, LOCAL_KEY_VALUE, type KeyCheck } from "../pages/apiKeyFormat";
import { baseUrlBlocks, baseUrlLabel, checkBaseUrl, normalizeBaseUrl } from "../pages/baseUrlFormat";
import { ModelField, useProviderModels, type ProviderModelsQuery } from "../pages/providerModels";
import type { ApiKeyStatus } from "../bridge/types";
import { t } from "../i18n";

type WizardState = {
  step: 1 | 2 | 3;
  provider: string;
  preset: CompatiblePreset | null;
  name: string;
  baseUrl: string;
  model: string;
  reuseKeyRef: string | null;
  /// `null` — the checkbox has not been touched, so a local endpoint decides
  /// for itself. An explicit answer wins over that guess.
  noKey: boolean | null;
  newKey: string;
  newKeyLabel: string;
  search: string;
};

export type ProfileWizardSeed = {
  provider?: string;
  preset?: CompatiblePreset;
  model?: string;
  baseUrl?: string;
  name?: string;
  startStep?: 1 | 2 | 3;
};

export type ProfileWizardResult = {
  profile: LlmProfile;
  newKey?: { ref: string; value: string; label: string };
};

function initialState(seed: ProfileWizardSeed | undefined): WizardState {
  const providerId = seed?.provider ?? PROVIDERS[0].id;
  const provider = PROVIDERS.find((p) => p.id === providerId) ?? PROVIDERS[0];
  const preset = seed?.preset ?? null;
  return {
    step: seed?.startStep ?? 1,
    provider: provider.id,
    preset,
    name: seed?.name ?? "",
    baseUrl: seed?.baseUrl ?? (provider.id === "opencode-go" ? OPENCODE_GO_BASE_URL : (preset?.baseUrl ?? "")),
    model: seed?.model ?? preset?.suggestedModel ?? provider.defaultModel,
    reuseKeyRef: null,
    noKey: null,
    newKey: "",
    newKeyLabel: "",
    search: "",
  };
}

/**
 * Whether closing the wizard would throw away something typed.
 *
 * Picking a card is one click to redo, and asking about it would train people
 * to dismiss the question without reading it. What is worth a stop is what was
 * entered by hand: every step past the first already carries a key, and the
 * blank carries an address from the very first screen.
 */
export function hasUnsavedInput({ step, isCustom, baseUrl }: { step: number; isCustom: boolean; baseUrl: string }): boolean {
  return step > 1 || (isCustom && baseUrl.trim() !== "");
}

/** An empty field is a hint, a malformed value is an error. */
function checkTone(level: "error" | "warn", raw: string): string {
  if (!raw.trim()) return "var(--ink-mute)";
  return level === "error" ? "var(--err)" : "var(--warn)";
}

export function ProfileWizard({ apiKeys, existingProfiles, seed, onClose, onCreate }: {
  apiKeys: ApiKeyStatus;
  existingProfiles: LlmProfile[];
  seed?: ProfileWizardSeed;
  onClose: () => void;
  onCreate: (next: ProfileWizardResult) => Promise<void>;
}) {
  const [state, setState] = useState<WizardState>(() => initialState(seed));
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // The question about closing lives in the wizard's own footer. `window.confirm`
  // was the first attempt — it is what the rest of the app deletes things with —
  // but in this webview it returns without ever drawing anything, so the modal
  // closed silently no matter what had been typed.
  const [confirmingClose, setConfirmingClose] = useState(false);
  const remoteModels = useProviderModels();

  function update(patch: Partial<WizardState>) {
    setState((current) => ({ ...current, ...patch }));
  }

  // A profile with no preset behind it: the same `compatible` provider the
  // presets use, only with every field left to the user. Until it existed, the
  // way to reach an unlisted provider was to take some preset and rewrite it
  // afterwards — and the profile went on carrying that preset name and its key
  // format hints.
  const isCustom = state.provider === "compatible" && !state.preset;

  function pickProvider(providerId: string) {
    const provider = PROVIDERS.find((p) => p.id === providerId) ?? PROVIDERS[0];
    update({
      provider: providerId,
      preset: null,
      baseUrl: providerId === "opencode-go" ? OPENCODE_GO_BASE_URL : "",
      model: provider.defaultModel,
      name: state.name || provider.name,
      noKey: null,
    });
  }

  function pickPreset(preset: CompatiblePreset) {
    update({
      provider: "compatible",
      preset,
      baseUrl: preset.baseUrl,
      model: preset.suggestedModel ?? state.model,
      name: state.name || `${preset.name} · ${preset.suggestedModel ?? "auto"}`,
      noKey: null,
    });
  }

  function pickCustom() {
    // Everything is cleared, the name included: a name carried over from the
    // card clicked a moment earlier would be a lie about where the profile
    // points. The default one is derived from the host when the profile is
    // created.
    update({ provider: "compatible", preset: null, baseUrl: "", model: "", name: "", noKey: null });
  }

  const availableKeys = useMemo(() => {
    return Object.entries(apiKeys)
      .filter(([, info]) => info.available)
      .map(([ref, info]) => ({ ref, info }));
  }, [apiKeys]);

  // The address is checked where it is typed. `llmRouteBlocker` sees a broken
  // Base URL as well, but only in a config that has already been written — a
  // typo used to turn into a profile that quietly processed nothing.
  const urlCheck = state.provider === "compatible" ? checkBaseUrl(state.baseUrl) : null;
  const urlBlocks = baseUrlBlocks(urlCheck);
  // An empty field is not a remark worth printing: the placeholder shows the
  // shape of the address and «Далее» stays disabled until there is one. The
  // check itself is still made — `submit` reports it if the field is emptied
  // on the last step.
  const urlNote = urlCheck && urlCheck.code !== "empty" ? urlCheck : null;

  // A local server accepts any token, so the key step has nothing to ask for.
  // The checkbox is offered only where that holds; an explicit answer wins, and
  // until there is one the endpoint decides.
  const keyOptional = keyIsOptional(state.preset?.id ?? null, state.baseUrl);
  const noKey = keyOptional && (state.noKey ?? true);
  const wantsNewKey = !state.reuseKeyRef;
  // Recomputed on every keystroke: the key step must not let through a value
  // that «Создать профиль» will refuse anyway — that used to be discovered two
  // steps later, already on the third screen.
  const keyCheck: KeyCheck = !noKey && wantsNewKey
    ? checkApiKey(state.provider, state.preset?.id ?? null, state.newKey)
    : null;
  const keyBlocks = apiKeyBlocks(keyCheck);

  // The address the request will actually go to — the field, or the constant
  // for the one provider that has its own. `submit` writes the same value.
  const baseUrl = normalizeBaseUrl(state.baseUrl) || (state.provider === "opencode-go" ? OPENCODE_GO_BASE_URL : "");
  // A key already in the store is passed by ref, as everywhere else. One typed
  // two screens ago has no ref yet, so it goes by value — that is what the
  // `api_key` argument of `fetch_provider_models` is for.
  const modelsQuery: ProviderModelsQuery = {
    provider: state.provider,
    baseUrl: baseUrl || undefined,
    apiKeyRef: state.reuseKeyRef ?? undefined,
    apiKey: noKey ? LOCAL_KEY_VALUE : state.newKey.trim() || undefined,
  };

  const query = state.search.trim().toLowerCase();
  function matches(...fields: Array<string | undefined>): boolean {
    return !query || fields.some((field) => field?.toLowerCase().includes(query));
  }
  const catalog = PROVIDER_CATALOG().filter((entry) => matches(
    entry.name,
    entry.id,
    entry.meta,
    entry.preset?.suggestedModel,
    entry.provider?.defaultModel,
  ));

  async function submit() {
    setError(null);
    if (!state.model.trim()) { setError(t("Введите Model ID.")); return; }
    if (urlBlocks) { setError(urlCheck?.message ?? t("Для OpenAI-compatible нужен Base URL.")); return; }
    if (keyBlocks) { setError(keyCheck?.message ?? t("Введите значение API-ключа или выберите существующий.")); return; }

    setSubmitting(true);
    try {
      const id = `profile_${Date.now().toString(36)}`;
      const taken = new Set(existingProfiles.map((p) => p.name));
      const finalName = (() => {
        const base = state.name.trim()
          // A hand-typed address has no brand to borrow a name from, and
          // «OpenAI-compatible» would be the name of every such profile.
          || (isCustom ? baseUrlLabel(baseUrl) : "")
          || (PROVIDERS.find((p) => p.id === state.provider)?.name ?? t("Новый профиль"));
        if (!taken.has(base)) return base;
        let i = 2;
        while (taken.has(`${base} (${i})`)) i++;
        return `${base} (${i})`;
      })();
      const apiKeyRef = state.reuseKeyRef ?? `key_${id}`;
      const profile: LlmProfile = {
        id,
        name: finalName,
        provider: state.provider,
        model: state.model.trim(),
        api_key_ref: apiKeyRef,
        prompt_preset: "plain",
        // Empty means "built-in". A copy of the text here would freeze in the
        // profile forever and never receive edits to the built-in prompt.
        system_prompt: "",
        base_url: baseUrl,
        llm_min_duration_seconds: 0,
        llm_timeout_seconds: 12,
      };
      const newKeyPayload = wantsNewKey
        ? {
            ref: apiKeyRef,
            value: noKey ? LOCAL_KEY_VALUE : state.newKey.trim(),
            label: state.newKeyLabel.trim() || finalName,
          }
        : undefined;
      await onCreate({ profile, newKey: newKeyPayload });
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSubmitting(false);
    }
  }

  function next() {
    if (state.step < 3) update({ step: (state.step + 1) as WizardState["step"] });
  }
  function prev() {
    if (state.step > 1) update({ step: (state.step - 1) as WizardState["step"] });
  }

  // The blank is the one card that says nothing by itself: with no address
  // there is nothing to carry to the key step, so it is asked for right here.
  const canNext = (state.step === 1 && !!state.provider && !(isCustom && urlBlocks))
    || (state.step === 2 && !keyBlocks);

  /** Every way out except «Создать профиль» — the X, the overlay, «Отмена». */
  function requestClose() {
    if (hasUnsavedInput({ step: state.step, isCustom, baseUrl: state.baseUrl })) {
      setConfirmingClose(true);
      return;
    }
    onClose();
  }

  return (
    <div className="modal-overlay" onMouseDown={(e) => { if (e.target === e.currentTarget) requestClose(); }}>
      <div className="modal modal--wide" role="dialog" aria-modal="true">
        {/* The counter goes beside the title, and the segments below take over
            the head's divider instead of lying on top of it: the window is 710px
            tall at its smallest, and two of those rows were spent on saying
            «шаг 1». */}
        <div className="modal__head modal__head--flush">
          <div className="modal__title">
            <h2>{t("Новый профиль LLM")}</h2>
            <span className="sub">{t("Шаг")} {state.step}  {t("из 3")}</span>
          </div>
          <button className="modal__close" onClick={requestClose} aria-label={t("Закрыть")}><Icon name="x" size={14}/></button>
        </div>
        <div className="wizard-steps">
          <div className="wizard-step" data-active={state.step === 1} data-done={state.step > 1}/>
          <div className="wizard-step" data-active={state.step === 2} data-done={state.step > 2}/>
          <div className="wizard-step" data-active={state.step === 3} data-done={false}/>
        </div>
        <div className="modal__body">
          {state.step === 1 && (
            <>
              {/* Nineteen cards is past the point where reading them all beats
                  typing three letters. The blank stays out of the filter on
                  purpose: it is the answer when nothing matched. */}
              <label className="input-search input-search--clearable wizard-search">
                <span className="input-search__icon"><Icon name="search" size={13}/></span>
                <input
                  className="field"
                  type="text"
                  placeholder={t("Поиск: провайдер, пресет, адрес…")}
                  value={state.search}
                  onChange={(e) => update({ search: e.target.value })}
                />
                {state.search && (
                  <button type="button" className="icon-btn input-search__clear" onClick={() => update({ search: "" })} aria-label={t("Очистить поиск")}>
                    <Icon name="x" size={12}/>
                  </button>
                )}
              </label>
              <div className="wizard-section">
                <div className="wizard-label">{t("Вручную")}</div>
                <button className="wizard-provider-card wizard-provider-card--custom" data-selected={isCustom} onClick={pickCustom}>
                  <LogoMark fallback="brand-compatible" color="var(--ink-dim)" size={22}/>
                  <div style={{ minWidth: 0 }}>
                    <div className="name">{t("Своя конфигурация")}</div>
                    <div className="meta">{t("Base URL и Model ID заполняются вручную, без пресета")}</div>
                  </div>
                </button>
                {/* The address is asked for here rather than on the third step:
                    the blank has nothing else to say about itself, and the key
                    step needs it to know whether a key is wanted at all. */}
                {isCustom && (
                  <label style={{ display: "grid", gap: 6, marginTop: 4 }}>
                    <span className="wizard-label">Base URL</span>
                    <input className="field mono wizard-url-field" value={state.baseUrl} onChange={(e) => update({ baseUrl: e.target.value })}
                      placeholder="https://api.example.com/v1" autoFocus
                      aria-invalid={urlBlocks} aria-describedby={urlNote ? "wizard-url-check" : undefined}/>
                    {urlNote && (
                      <div id="wizard-url-check" role={urlNote.level === "error" ? "alert" : "status"}
                        style={{ font: "500 12px/1.4 var(--font-sans)", color: checkTone(urlNote.level, state.baseUrl) }}>
                        {urlNote.message}
                      </div>
                    )}
                  </label>
                )}
              </div>
              {/* Grouped by what the entry is to whoever is choosing it, not by
                  which adapter serves it — see `catalogGroup`. Name and logo
                  are the whole card: the default model was a fact about the
                  third step and cost the row half its cards, and the address
                  earns its place only where it points at your own machine. */}
              {CATALOG_GROUPS().map(({ id: group, label }) => {
                const entries = catalog.filter((entry) => entry.group === group);
                if (entries.length === 0) return null;
                return (
                  <div className="wizard-section" key={group}>
                    <div className="wizard-label">{label}</div>
                    <div className={group === "local" ? "wizard-provider-grid" : "wizard-provider-grid wizard-provider-grid--compact"}>
                      {entries.map((entry) => (
                        <button key={entry.id} className="wizard-provider-card"
                          data-selected={entry.preset ? state.preset?.id === entry.id : state.provider === entry.id && !state.preset}
                          onClick={() => (entry.preset ? pickPreset(entry.preset) : pickProvider(entry.id))}>
                          <LogoMark logo={entry.logo} fallback={entry.icon} color={entry.color} size={22}/>
                          {group === "local" ? (
                            <div style={{ minWidth: 0 }}>
                              <div className="name">{entry.name}</div>
                              <div className="meta">{entry.meta}</div>
                            </div>
                          ) : (
                            <div className="name">{entry.name}</div>
                          )}
                        </button>
                      ))}
                    </div>
                  </div>
                );
              })}
              {catalog.length === 0 && (
                <div style={{ font: "500 12px/1.4 var(--font-sans)", color: "var(--ink-mute)" }}>
                  {t("Ничего не найдено — заполните адрес вручную.")}
                </div>
              )}
            </>
          )}
          {state.step === 2 && (
            <>
              <div className="wizard-label">{t("API-ключ")}</div>
              {/* A local server checks nothing, and until this box existed the
                  wizard demanded a key it would never send anywhere: LM Studio,
                  Ollama and vLLM could not be set up through it at all. */}
              {keyOptional && (
                <label className="checkbox-row wizard-check">
                  <input className="checkbox" type="checkbox" checked={noKey} onChange={(e) => update({ noKey: e.target.checked })}/>
                  <span style={{ minWidth: 0 }}>
                    <span className="name">{t("Ключ не нужен — сервер локальный")}</span>
                    <span className="meta">{t("В слот запишется «local»: локальный сервер токен не проверяет, а пустой слот выключил бы обработку.")}</span>
                  </span>
                </label>
              )}
              {!noKey && (
                <>
                  {availableKeys.length > 0 && (
                    <div style={{ display: "grid", gap: 6 }}>
                      <span style={{ font: "500 11px/1.4 var(--font-mono)", color: "var(--ink-mute)" }}>{t("Использовать существующий слот:")}</span>
                      <CustomSelect<string>
                        value={state.reuseKeyRef ?? ""}
                        inlineMeta
                        options={[
                          { value: "", label: t("— Новый ключ —") },
                          ...availableKeys.map<SelectOption<string>>(({ ref, info }) => ({
                            value: ref,
                            label: info.label || ref,
                            meta: info.masked,
                          })),
                        ]}
                        onChange={(next) => update({ reuseKeyRef: next || null })}
                      />
                    </div>
                  )}
                  {!state.reuseKeyRef && (
                    <>
                      <label style={{ display: "grid", gap: 6 }}>
                        <span className="wizard-label">{t("Метка ключа (опционально)")}</span>
                        <input className="field" value={state.newKeyLabel} onChange={(e) => update({ newKeyLabel: e.target.value })} placeholder={state.name || t("Например: Cerebras gpt-oss")}/>
                      </label>
                      <label style={{ display: "grid", gap: 6 }}>
                        <span className="wizard-label">{t("Значение")}</span>
                        <input className="field mono" type="password" value={state.newKey} onChange={(e) => update({ newKey: e.target.value })} placeholder="sk-..."
                          aria-invalid={keyBlocks} aria-describedby={keyCheck ? "wizard-key-check" : undefined}/>
                      </label>
                      {/* The line is always there when there is something to say:
                          «Далее» is disabled, and a silent grey button is the same
                          dead end as a refusal on the third step. An empty field is
                          not yet the user's mistake, so it is a hint, not red. */}
                      {keyCheck && (
                        <div id="wizard-key-check" role={keyCheck.level === "error" && state.newKey.trim() ? "alert" : "status"}
                          style={{ font: "500 12px/1.4 var(--font-sans)", color: checkTone(keyCheck.level, state.newKey) }}>
                          {keyCheck.message}
                        </div>
                      )}
                    </>
                  )}
                </>
              )}
            </>
          )}
          {state.step === 3 && (
            <>
              <label style={{ display: "grid", gap: 6 }}>
                <span className="wizard-label">{t("Название профиля")}</span>
                <input className="field" value={state.name} onChange={(e) => update({ name: e.target.value })}
                  placeholder={(isCustom && baseUrlLabel(state.baseUrl)) || t("Например: Cerebras gpt-oss-120b")} maxLength={64}/>
              </label>
              {state.provider === "compatible" && (
                <label style={{ display: "grid", gap: 6 }}>
                  <span className="wizard-label">Base URL</span>
                  <input className="field mono wizard-url-field" value={state.baseUrl} onChange={(e) => update({ baseUrl: e.target.value })} placeholder="https://api.example.com/v1"
                    aria-invalid={urlBlocks} aria-describedby={urlNote ? "wizard-url-review" : undefined}/>
                  {urlNote && (
                    <div id="wizard-url-review" role={urlNote.level === "error" ? "alert" : "status"}
                      style={{ font: "500 12px/1.4 var(--font-sans)", color: checkTone(urlNote.level, state.baseUrl) }}>
                      {urlNote.message}
                    </div>
                  )}
                </label>
              )}
              {/* The same field as on «Интеграции», refresh button and all: the
                  list a provider serves today beats one compiled into the app
                  months ago, and it is worth more here than anywhere — this is
                  where the id is chosen for the first time. Where the docs are
                  to be found is a footnote about the field, so it hangs off the
                  caption instead of taking a row under it. */}
              <label style={{ display: "grid", gap: 6 }}>
                <span className="wizard-label">
                  Model ID <Hint text={MODEL_HINTS()[state.provider] ?? t("Model ID берётся из документации провайдера.")}/>
                </span>
                <ModelField
                  cacheKey={`wizard:${state.provider}:${baseUrl}`}
                  value={state.model}
                  onChange={(v) => update({ model: v })}
                  fallbackSuggestions={PROVIDER_MODEL_OPTIONS[state.provider] ?? []}
                  placeholder={t("например: gpt-oss-120b")}
                  query={modelsQuery}
                  state={remoteModels}
                />
              </label>
              {error && <div style={{ color: "var(--err)", font: "500 12px/1.4 var(--font-sans)" }}>{error}</div>}
            </>
          )}
        </div>
        <div className="modal__foot">
          <button className="btn btn--ghost" onClick={state.step === 1 ? requestClose : prev}>{state.step === 1 ? t("Отмена") : t("Назад")}</button>
          {state.step < 3 ? (
            <button className="btn btn--primary" onClick={next} disabled={!canNext}><Icon name="arrow-right" size={12}/>{t("Далее")}</button>
          ) : (
            <button className="btn btn--primary" onClick={() => void submit()} disabled={submitting}><Icon name="check" size={12}/>{submitting ? t("Создаю…") : t("Создать профиль")}</button>
          )}
        </div>
      </div>
      {/* A window of its own over the wizard, not a line in its footer: the
          question is asked about the window still standing behind it, and it
          has to be answered before anything else can be clicked. «Остаться» is
          the primary button — the safe answer is the one under the finger, and
          clicking the backdrop means the same thing. */}
      {confirmingClose && (
        <div className="modal-overlay modal-overlay--stacked" onMouseDown={(e) => { if (e.target === e.currentTarget) setConfirmingClose(false); }}>
          <div className="modal modal--ask" role="alertdialog" aria-modal="true" aria-labelledby="wizard-close-title">
            <div className="modal__head">
              <div className="modal__title"><h2 id="wizard-close-title">{t("Закрыть мастер?")}</h2></div>
            </div>
            <div className="modal__body">
              <div style={{ font: "500 12.5px/1.45 var(--font-sans)", color: "var(--ink-dim)" }}>
                {t("Введённые данные не сохранятся.")}
              </div>
            </div>
            <div className="modal__foot">
              <button className="btn btn--ghost" onClick={onClose}>{t("Закрыть без сохранения")}</button>
              <button className="btn btn--primary" autoFocus onClick={() => setConfirmingClose(false)}>{t("Остаться")}</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
