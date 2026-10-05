import { useMemo, useRef, useState } from "react";
import { Icon } from "./Icon";
import { CustomSelect, type SelectOption } from "./CustomSelect";
import { FieldCheckNote } from "./FieldCheckNote";
import { Hint } from "./Hint";
import { Modal } from "./Modal";
import {
  CATALOG_GROUPS,
  LogoMark,
  MODEL_HINTS,
  OPENCODE_GO_BASE_URL,
  PROVIDERS,
  PROVIDER_CATALOG,
  type CompatiblePreset,
  type LlmProfile,
} from "../pages/aiShared";
import { apiKeyBlocks, checkApiKey, keyIsOptional, LOCAL_KEY_VALUE, type KeyCheck } from "../pages/apiKeyFormat";
import { baseUrlBlocks, baseUrlLabel, checkBaseUrl, normalizeBaseUrl, type UrlCheck } from "../pages/baseUrlFormat";
import { ModelField, useProviderModels, type ProviderModelsQuery } from "../pages/providerModels";
import type { ApiKeyStatus } from "../bridge/types";
import { t } from "../i18n";

type WizardState = {
  step: 1 | 2 | 3;
  /// Empty until a card is picked: a card lit up before anyone chose it was a
  /// choice the user had not made, one «Далее» away from being a profile.
  provider: string;
  preset: CompatiblePreset | null;
  name: string;
  /// The name was typed rather than derived from the card. A derived one
  /// follows the card; a typed one is never overwritten.
  nameEdited: boolean;
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
  const provider = PROVIDERS.find((p) => p.id === seed?.provider);
  const preset = seed?.preset ?? null;
  return {
    step: seed?.startStep ?? 1,
    provider: provider?.id ?? "",
    preset,
    name: seed?.name ?? "",
    nameEdited: Boolean(seed?.name),
    baseUrl: seed?.baseUrl ?? (provider?.id === "opencode-go" ? OPENCODE_GO_BASE_URL : (preset?.baseUrl ?? "")),
    // No model until one is picked: an id compiled into the app goes stale
    // between releases, and the provider's own list is a click away.
    model: seed?.model ?? "",
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

/** The address field, asked for on the first step for the blank card and
 *  shown again on the last one for every OpenAI-compatible entry. */
function BaseUrlField({ id, value, check, onChange, autoFocus }: {
  id: string;
  value: string;
  check: UrlCheck;
  onChange: (next: string) => void;
  autoFocus?: boolean;
}) {
  // An empty field is not a remark worth printing: the placeholder shows the
  // shape of the address and «Далее» stays disabled until there is one. The
  // check itself is still made — `submit` reports it if the field is emptied
  // on the last step.
  const note = check && check.code !== "empty" ? check : null;
  return (
    <label className="wizard-field">
      <span className="wizard-label">Base URL</span>
      <input className="field mono wizard-url-field" aria-label="Base URL" value={value} onChange={(e) => onChange(e.target.value)}
        placeholder="https://api.example.com/v1" autoFocus={autoFocus}
        aria-invalid={baseUrlBlocks(check)} aria-describedby={note ? id : undefined}/>
      <FieldCheckNote id={id} check={note} value={value}/>
    </label>
  );
}

export function ProfileWizard({ apiKeys, existingProfiles, seed, onClose, onCreate }: {
  apiKeys: ApiKeyStatus;
  existingProfiles: LlmProfile[];
  seed?: ProfileWizardSeed;
  onClose: () => void;
  onCreate: (next: ProfileWizardResult) => Promise<void>;
}) {
  const createId = useRef<string | null>(null);
  const [state, setState] = useState<WizardState>(() => initialState(seed));
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // The question about closing is asked in the wizard's own dialog.
  // `window.confirm` was the first attempt, but in this webview it returns
  // without ever drawing anything, so the modal closed silently no matter what
  // had been typed.
  const [confirmingClose, setConfirmingClose] = useState(false);
  const [keyRevealed, setKeyRevealed] = useState(false);
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

  /** What a different card resets. A derived name follows the card — kept,
   *  it named the profile after whichever card was clicked first — and a key
   *  chosen or typed for one provider does not authenticate with another. */
  function switchTo(selection: string, derivedName: string): Partial<WizardState> {
    const current = state.preset?.id ?? state.provider;
    return {
      name: state.nameEdited ? state.name : derivedName,
      noKey: null,
      ...(selection === current ? {} : { reuseKeyRef: null, newKey: "", newKeyLabel: "" }),
    };
  }

  function pickProvider(providerId: string) {
    const provider = PROVIDERS.find((p) => p.id === providerId) ?? PROVIDERS[0];
    update({
      ...switchTo(providerId, provider.name),
      provider: providerId,
      preset: null,
      baseUrl: providerId === "opencode-go" ? OPENCODE_GO_BASE_URL : "",
      model: "",
    });
  }

  function pickPreset(preset: CompatiblePreset) {
    update({
      ...switchTo(preset.id, preset.name),
      provider: "compatible",
      preset,
      baseUrl: preset.baseUrl,
      model: "",
    });
  }

  function pickCustom() {
    // The derived name goes too: the default for a hand-typed address is its
    // host, taken when the profile is created.
    update({ ...switchTo("compatible", ""), provider: "compatible", preset: null, baseUrl: "", model: "" });
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
  ));

  async function submit() {
    setError(null);
    if (!state.model.trim()) { setError(t("Введите Model ID.")); return; }
    if (urlBlocks) { setError(urlCheck?.message ?? t("Для OpenAI-compatible нужен Base URL.")); return; }
    if (keyBlocks) { setError(keyCheck?.message ?? t("Введите значение API-ключа или выберите существующий.")); return; }

    setSubmitting(true);
    try {
      // A failed config write leaves the wizard open. Reusing the id also
      // reuses the key ref already written to the OS store on the first try.
      const id = createId.current ??= `profile_${Date.now().toString(36)}`;
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

  /** Every way out except «Создать профиль» — the X, Escape, the overlay.
   *  While the question is on screen, the same ways out answer «Остаться». */
  function requestClose() {
    if (confirmingClose) { setConfirmingClose(false); return; }
    if (hasUnsavedInput({ step: state.step, isCustom, baseUrl: state.baseUrl })) {
      setConfirmingClose(true);
      return;
    }
    onClose();
  }

  const stepNames = [t("Провайдер"), t("API-ключ"), t("Модель")];

  // The question replaces the wizard inside the same dialog, as the dictionary
  // editor asks it: a second overlay stacked on top had its own Escape and
  // focus rules to keep in step with the first, and kept neither.
  if (confirmingClose) {
    return (
      <Modal title={t("Закрыть мастер?")} onClose={requestClose} showHeader={false} className="modal--ask">
        <div className="modal__head">
          <div className="modal__title"><h2>{t("Закрыть мастер?")}</h2></div>
        </div>
        <div className="modal__body">
          <p className="dictionary-note">{t("Введённые данные не сохранятся.")}</p>
        </div>
        {/* «Остаться» is the primary button: the safe answer is the one under
            the finger, and Escape or a click outside mean the same thing. */}
        <div className="modal__foot">
          <button className="btn btn--ghost" onClick={onClose}>{t("Закрыть без сохранения")}</button>
          <button className="btn btn--primary" autoFocus onClick={() => setConfirmingClose(false)}>{t("Остаться")}</button>
        </div>
      </Modal>
    );
  }

  return (
    // The first step is a choice of card, not something to type: a focused
    // search box lit up as if it were the thing to do first.
    <Modal title={t("Новый профиль LLM")} onClose={requestClose} busy={submitting} showHeader={false} focusFirstInput={false} className="modal--wide">
        {/* The counter goes beside the title, and the segments below take over
            the head's divider instead of lying on top of it: the window is 710px
            tall at its smallest, and two of those rows were spent on saying
            «шаг 1». */}
        <div className="modal__head modal__head--flush">
          <div className="modal__title">
            <h2>{t("Новый профиль LLM")}</h2>
            <span className="sub">{t("Шаг")} {state.step}  {t("из 3")} · {stepNames[state.step - 1]}</span>
          </div>
          <button className="modal__close" onClick={requestClose} disabled={submitting} aria-label={t("Закрыть")}><Icon name="x" size={14}/></button>
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
                  <BaseUrlField id="wizard-url-check" value={state.baseUrl} check={urlCheck} autoFocus
                    onChange={(baseUrl) => update({ baseUrl })}/>
                )}
              </div>
              {/* Grouped by what the entry is to whoever is choosing it, not by
                  which adapter serves it — see `catalogGroup`. Name and logo
                  are the whole card: the default model was a fact about the
                  third step and cost the row half its cards, and the address
                  earns its place only where it points at your own machine —
                  as host and port, which is what gets checked against the
                  server running there. */}
              {CATALOG_GROUPS().map(({ id: group, label }) => {
                const entries = catalog.filter((entry) => entry.group === group);
                if (entries.length === 0) return null;
                return (
                  <div className="wizard-section" key={group}>
                    <div className="wizard-label">{label}</div>
                    <div className="wizard-provider-grid">
                      {entries.map((entry) => (
                        <button key={entry.id} className="wizard-provider-card"
                          data-selected={entry.preset ? state.preset?.id === entry.id : state.provider === entry.id && !state.preset}
                          onClick={() => (entry.preset ? pickPreset(entry.preset) : pickProvider(entry.id))}>
                          <LogoMark logo={entry.logo} fallback={entry.icon} color={entry.color} size={22}/>
                          {group === "local" ? (
                            <div style={{ minWidth: 0 }}>
                              <div className="name">{entry.name}</div>
                              <div className="meta">{baseUrlLabel(entry.meta)}</div>
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
                <div className="field-check">{t("Ничего не найдено — заполните адрес вручную.")}</div>
              )}
            </>
          )}
          {state.step === 2 && (
            <>
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
                    <div className="wizard-field">
                      <span className="wizard-label">{t("Использовать существующий слот:")}</span>
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
                      <label className="wizard-field">
                        <span className="wizard-label">{t("Метка ключа (опционально)")}</span>
                        <input className="field" value={state.newKeyLabel} onChange={(e) => update({ newKeyLabel: e.target.value })} placeholder={state.name || t("Например: Cerebras gpt-oss")}/>
                      </label>
                      <label className="wizard-field">
                        <span className="wizard-label">{t("Значение")}</span>
                        <div style={{ display: "flex", gap: 6 }}>
                          <input className="field mono" type={keyRevealed ? "text" : "password"} value={state.newKey} onChange={(e) => update({ newKey: e.target.value })} placeholder="sk-..."
                            style={{ flex: 1 }} aria-invalid={keyBlocks} aria-describedby={keyCheck ? "wizard-key-check" : undefined}/>
                          <Hint asChild text={keyRevealed ? t("Скрыть ключ") : t("Показать ключ")}>
                            <button
                              type="button"
                              className="btn btn--icon field-reveal"
                              onClick={() => setKeyRevealed((v) => !v)}
                              aria-label={keyRevealed ? t("Скрыть ключ") : t("Показать ключ")}
                            >
                              <Icon name={keyRevealed ? "eye-off" : "eye"} size={13}/>
                            </button>
                          </Hint>
                        </div>
                      </label>
                      {/* The line is always there when there is something to say:
                          «Далее» is disabled, and a silent grey button is the same
                          dead end as a refusal on the third step. An empty field is
                          not yet the user's mistake, so it is a hint, not red. */}
                      <FieldCheckNote id="wizard-key-check" check={keyCheck} value={state.newKey}/>
                    </>
                  )}
                </>
              )}
            </>
          )}
          {state.step === 3 && (
            <>
              <label className="wizard-field">
                <span className="wizard-label">{t("Название профиля")}</span>
                <input className="field" value={state.name} onChange={(e) => update({ name: e.target.value, nameEdited: e.target.value !== "" })}
                  placeholder={(isCustom && baseUrlLabel(state.baseUrl)) || t("Например: Cerebras gpt-oss-120b")} maxLength={64}/>
              </label>
              {state.provider === "compatible" && (
                <BaseUrlField id="wizard-url-review" value={state.baseUrl} check={urlCheck}
                  onChange={(baseUrl) => update({ baseUrl })}/>
              )}
              {/* The same field as on «Интеграции», refresh button and all: the
                  list a provider serves today beats one compiled into the app
                  months ago, and it is worth more here than anywhere — this is
                  where the id is chosen for the first time. Where the docs are
                  to be found is a footnote about the field, so it hangs off the
                  caption instead of taking a row under it. */}
              <label className="wizard-field">
                <span className="wizard-label">
                  Model ID <Hint text={MODEL_HINTS()[state.provider] ?? t("Model ID берётся из документации провайдера.")}/>
                </span>
                <ModelField
                  cacheKey={`wizard:${state.provider}:${baseUrl}`}
                  value={state.model}
                  onChange={(v) => update({ model: v })}
                  placeholder={t("Выберите из списка провайдера или введите id")}
                  query={modelsQuery}
                  state={remoteModels}
                />
              </label>
              {error && <div className="field-check" data-tone="error" role="alert">{error}</div>}
            </>
          )}
        </div>
        <div className="modal__foot">
          {/* Step one has nothing to go back to, and leaving is what the
              cross in the header already does. */}
          {state.step === 1 ? <span/> : <button className="btn btn--ghost" onClick={prev}>{t("Назад")}</button>}
          {state.step < 3 ? (
            <button className="btn btn--primary" onClick={next} disabled={!canNext}><Icon name="arrow-right" size={12}/>{t("Далее")}</button>
          ) : (
            <button className="btn btn--primary" onClick={() => void submit()} disabled={submitting}><Icon name="check" size={12}/>{submitting ? t("Создаю…") : t("Создать профиль")}</button>
          )}
        </div>
    </Modal>
  );
}
