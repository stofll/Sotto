import { useMemo, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { Icon } from "./Icon";
import { useOutsideClose } from "./CustomSelect";
import { useAnchoredMenu } from "./anchoredMenu";
import {
  COMPATIBLE_PRESETS,
  LogoMark,
  MODEL_HINTS,
  OPENCODE_GO_BASE_URL,
  PROVIDERS,
  PROVIDER_MODEL_OPTIONS,
  ProviderMark,
  type CompatiblePreset,
  type LlmProfile,
} from "../pages/aiShared";
import { apiKeyBlocks, checkApiKey, type KeyCheck } from "../pages/apiKeyFormat";
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
  newKey: string;
  newKeyLabel: string;
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
    newKey: "",
    newKeyLabel: "",
  };
}

function ModelCombobox({ value, suggestions, onChange, onCommit, placeholder }: {
  value: string;
  suggestions: string[];
  onChange: (next: string) => void;
  onCommit: () => void;
  placeholder?: string;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const { menuRef, style: menuStyle } = useAnchoredMenu(open, rootRef, 240);
  useOutsideClose(open, rootRef, () => setOpen(false), menuRef);

  const filtered = useMemo(() => {
    const q = value.trim().toLowerCase();
    if (!q) return suggestions;
    return suggestions.filter((s) => s.toLowerCase().includes(q));
  }, [value, suggestions]);

  const menu = (body: ReactNode) => createPortal(
    <div className="combobox__menu" role="listbox" ref={menuRef} style={menuStyle}>{body}</div>,
    document.body,
  );

  return (
    <div className="combobox" ref={rootRef}>
      <input
        className="field mono"
        value={value}
        onChange={(e) => { onChange(e.target.value); if (!open) setOpen(true); }}
        onFocus={() => setOpen(true)}
        onBlur={() => { window.setTimeout(() => { setOpen(false); onCommit(); }, 120); }}
        onKeyDown={(e) => {
          if (e.key === "Enter") { setOpen(false); onCommit(); }
          if (e.key === "Escape") setOpen(false);
        }}
        placeholder={placeholder}
        style={{ width: "100%", height: 34 }}
      />
      {open && menu(filtered.length > 0
        ? filtered.map((s) => (
          <button
            key={s}
            type="button"
            className="combobox__option"
            aria-selected={s === value}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => { onChange(s); setOpen(false); onCommit(); }}
          >
            {s}
            {s === value && <span className="meta"><Icon name="check" size={12}/></span>}
          </button>
        ))
        : <div className="combobox__empty">{t("Нет известных моделей — введите id вручную.")}</div>
      )}
    </div>
  );
}

/** Пустое поле — подсказка, испорченное значение — ошибка. */
function keyTone(check: NonNullable<KeyCheck>, raw: string): string {
  if (!raw.trim()) return "var(--text-mute)";
  return check.level === "error" ? "var(--err)" : "var(--warn)";
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

  function update(patch: Partial<WizardState>) {
    setState((current) => ({ ...current, ...patch }));
  }

  function pickProvider(providerId: string) {
    const provider = PROVIDERS.find((p) => p.id === providerId) ?? PROVIDERS[0];
    update({
      provider: providerId,
      preset: null,
      baseUrl: providerId === "opencode-go" ? OPENCODE_GO_BASE_URL : "",
      model: provider.defaultModel,
      name: state.name || provider.name,
    });
  }

  function pickPreset(preset: CompatiblePreset) {
    update({
      provider: "compatible",
      preset,
      baseUrl: preset.baseUrl,
      model: preset.suggestedModel ?? state.model,
      name: state.name || `${preset.name} · ${preset.suggestedModel ?? "auto"}`,
    });
  }

  const availableKeys = useMemo(() => {
    return Object.entries(apiKeys)
      .filter(([, info]) => info.available)
      .map(([ref, info]) => ({ ref, info }));
  }, [apiKeys]);

  const wantsNewKey = !state.reuseKeyRef;
  // Считается на каждый ввод: шаг ключа не должен отпускать дальше значение,
  // на котором «Создать профиль» всё равно откажет — раньше об этом узнавали
  // через два шага, уже на третьем экране.
  const keyCheck = wantsNewKey ? checkApiKey(state.provider, state.preset?.id ?? null, state.newKey) : null;
  const keyBlocks = apiKeyBlocks(keyCheck);

  async function submit() {
    setError(null);
    if (!state.model.trim()) { setError(t("Введите Model ID.")); return; }
    if (state.provider === "compatible" && !state.baseUrl.trim()) { setError(t("Для OpenAI-compatible нужен Base URL.")); return; }
    if (keyBlocks) { setError(keyCheck?.message ?? t("Введите значение API-ключа или выберите существующий.")); return; }

    setSubmitting(true);
    try {
      const id = `profile_${Date.now().toString(36)}`;
      const taken = new Set(existingProfiles.map((p) => p.name));
      const finalName = (() => {
        const base = state.name.trim() || (PROVIDERS.find((p) => p.id === state.provider)?.name ?? t("Новый профиль"));
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
        // Пусто — значит «встроенный». Копия текста здесь застывала бы в
        // профиле навсегда и не получала правок встроенного промпта.
        system_prompt: "",
        base_url: state.baseUrl.trim() || (state.provider === "opencode-go" ? OPENCODE_GO_BASE_URL : ""),
        llm_min_duration_seconds: 0,
        llm_timeout_seconds: 12,
      };
      const newKeyPayload = wantsNewKey
        ? { ref: apiKeyRef, value: state.newKey.trim(), label: state.newKeyLabel.trim() || finalName }
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

  const canNext = (state.step === 1 && !!state.provider) || (state.step === 2 && !keyBlocks);

  return (
    <div className="modal-overlay" onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div className="modal" role="dialog" aria-modal="true">
        <div className="modal__head">
          <div>
            <h2>{t("Новый профиль LLM")}</h2>
            <div className="sub">{t("Шаг")} {state.step}  {t("из 3")}</div>
          </div>
          <button className="modal__close" onClick={onClose} aria-label={t("Закрыть")}><Icon name="x" size={14}/></button>
        </div>
        <div className="wizard-steps">
          <div className="wizard-step" data-active={state.step === 1} data-done={state.step > 1}/>
          <div className="wizard-step" data-active={state.step === 2} data-done={state.step > 2}/>
          <div className="wizard-step" data-active={state.step === 3} data-done={false}/>
        </div>
        <div className="modal__body">
          {state.step === 1 && (
            <>
              <div className="wizard-label">{t("Прямой провайдер")}</div>
              <div className="wizard-provider-grid">
                {PROVIDERS.filter((p) => p.id !== "compatible").map((p) => (
                  <button key={p.id} className="wizard-provider-card" data-selected={state.provider === p.id && !state.preset}
                    onClick={() => pickProvider(p.id)}>
                    <ProviderMark provider={p} size={18}/>
                    <div style={{ minWidth: 0 }}>
                      <div className="name">{p.name}</div>
                      <div className="meta">{t("по умолчанию:")} {p.defaultModel}</div>
                    </div>
                  </button>
                ))}
              </div>
              <div className="wizard-label" style={{ marginTop: 6 }}>{t("OpenAI-compatible пресеты")}</div>
              <div className="wizard-provider-grid">
                {COMPATIBLE_PRESETS().map((p) => (
                  <button key={p.id} className="wizard-provider-card" data-selected={state.preset?.id === p.id}
                    onClick={() => pickPreset(p)}>
                    <LogoMark logo={p.logo} fallback="brand-compatible" color="var(--text-2)" size={18}/>
                    <div style={{ minWidth: 0 }}>
                      <div className="name">{p.name}</div>
                      <div className="meta">{p.baseUrl}</div>
                    </div>
                  </button>
                ))}
              </div>
            </>
          )}
          {state.step === 2 && (
            <>
              <div className="wizard-label">{t("API-ключ")}</div>
              {availableKeys.length > 0 && (
                <div style={{ display: "grid", gap: 6 }}>
                  <span style={{ font: "500 11px/1.4 var(--font-mono)", color: "var(--text-mute)" }}>{t("Использовать существующий слот:")}</span>
                  <select className="field" value={state.reuseKeyRef ?? ""} onChange={(e) => update({ reuseKeyRef: e.target.value || null })} style={{ height: 36 }}>
                    <option value="">{t("— Новый ключ —")}</option>
                    {availableKeys.map(({ ref, info }) => (
                      <option key={ref} value={ref}>{info.label || ref} · {info.masked}</option>
                    ))}
                  </select>
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
                  {/* Строка есть всегда, когда есть что сказать: «Далее»
                      заблокирована, и молчаливая серая кнопка — тот же тупик,
                      что и отказ на третьем шаге. Пустое поле при этом ещё не
                      ошибка пользователя, поэтому оно подсказка, а не красное. */}
                  {keyCheck && (
                    <div id="wizard-key-check" role={keyCheck.level === "error" && state.newKey.trim() ? "alert" : "status"}
                      style={{ font: "500 12px/1.4 var(--font-sans)", color: keyTone(keyCheck, state.newKey) }}>
                      {keyCheck.message}
                    </div>
                  )}
                </>
              )}
            </>
          )}
          {state.step === 3 && (
            <>
              <label style={{ display: "grid", gap: 6 }}>
                <span className="wizard-label">{t("Название профиля")}</span>
                <input className="field" value={state.name} onChange={(e) => update({ name: e.target.value })} placeholder={t("Например: Cerebras gpt-oss-120b")} maxLength={64}/>
              </label>
              {state.provider === "compatible" && (
                <label style={{ display: "grid", gap: 6 }}>
                  <span className="wizard-label">Base URL</span>
                  <input className="field mono" value={state.baseUrl} onChange={(e) => update({ baseUrl: e.target.value })} placeholder="https://api.example.com/v1"/>
                </label>
              )}
              <label style={{ display: "grid", gap: 6 }}>
                <span className="wizard-label">Model ID</span>
                <ModelCombobox
                  value={state.model}
                  suggestions={PROVIDER_MODEL_OPTIONS[state.provider] ?? []}
                  onChange={(v) => update({ model: v })}
                  onCommit={() => {}}
                  placeholder={t("например: gpt-oss-120b")}
                />
              </label>
              <div style={{ font: "500 11px/1.45 var(--font-mono)", color: "var(--text-mute)" }}>
                {MODEL_HINTS()[state.provider] ?? t("Model ID берётся из документации провайдера.")}
              </div>
              {error && <div style={{ color: "var(--err)", font: "500 12px/1.4 var(--font-sans)" }}>{error}</div>}
            </>
          )}
        </div>
        <div className="modal__foot">
          <button className="btn btn--ghost" onClick={state.step === 1 ? onClose : prev}>{state.step === 1 ? t("Отмена") : t("Назад")}</button>
          {state.step < 3 ? (
            <button className="btn btn--primary" onClick={next} disabled={!canNext}><Icon name="arrow-right" size={12}/>{t("Далее")}</button>
          ) : (
            <button className="btn btn--primary" onClick={() => void submit()} disabled={submitting}><Icon name="check" size={12}/>{submitting ? t("Создаю…") : t("Создать профиль")}</button>
          )}
        </div>
      </div>
    </div>
  );
}
