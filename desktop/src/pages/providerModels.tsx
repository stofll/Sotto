// The «Model» field with a list the provider serves itself.
//
// The id used to be typed by hand while the hint sent you off to read the
// documentation. Providers rename and retire models more often than the app
// ships releases, so a list hardcoded in the source goes stale silently — and
// you find out in the middle of a dictation, when you least want to
// investigate.
//
// The request goes out only on an explicit action: on switching provider and on
// the refresh button. No background polling — this is a network request made
// with the user's key. The field stays free for typing: ids entered by hand
// earlier must keep working even if the provider no longer lists them.

import { useCallback, useEffect, useState } from "react";
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { Icon } from "../components/Icon";
import { Hint } from "../components/Hint";
import { ModelCombobox } from "../components/ModelCombobox";
import { t } from "../i18n";

export interface ProviderModelsQuery {
    provider: string;
    baseUrl?: string;
    apiKeyRef?: string;
    /// A key that has no ref yet — the wizard has it typed but not saved. The
    /// stored key wins when the ref resolves to one; see `model_request_key`.
    apiKey?: string;
}

/** How long «вариантов от провайдера: N» stays on screen. It is a receipt for
 *  a button press, not a property of the field: the list itself is kept, and
 *  the caption's job is done as soon as it has been read. */
const COUNT_TTL_MS = 6000;

export interface ProviderModelsState {
    /// Fetched lists by cache key (see `cacheKey` in ModelField).
    models: Record<string, string[]>;
    /// When each list arrived. Lives here rather than in the field so that
    /// collapsing and reopening a profile does not replay a receipt for an
    /// action taken ten minutes ago.
    loadedAt: Record<string, number>;
    loadingKey: string | null;
    errors: Record<string, string>;
    load: (cacheKey: string, query: ProviderModelsQuery) => Promise<void>;
}

export function useProviderModels(): ProviderModelsState {
    const [models, setModels] = useState<Record<string, string[]>>({});
    const [loadedAt, setLoadedAt] = useState<Record<string, number>>({});
    const [loadingKey, setLoadingKey] = useState<string | null>(null);
    const [errors, setErrors] = useState<Record<string, string>>({});

    const load = useCallback(async (cacheKey: string, query: ProviderModelsQuery) => {
        setLoadingKey(cacheKey);
        setErrors((current) => {
            const next = { ...current };
            delete next[cacheKey];
            return next;
        });
        // The previous answer goes too, not only the previous error. A reload
        // is most often a change of provider, and until the new list arrives
        // the old one is not «stale», it is somebody else's: the caption would
        // keep counting options nobody offers any more, and if the request
        // failed — no key for the new provider is the ordinary case — the field
        // would go on suggesting the previous provider's models. With it gone,
        // the field falls back to the ids compiled into the app.
        setModels((current) => {
            if (!(cacheKey in current)) return current;
            const next = { ...current };
            delete next[cacheKey];
            return next;
        });
        try {
            const list = await tauriInvoke<string[]>("fetch_provider_models", {
                provider: query.provider,
                base_url: query.baseUrl ?? null,
                api_key_ref: query.apiKeyRef ?? null,
                api_key: query.apiKey ?? null,
            });
            setModels((current) => ({ ...current, [cacheKey]: list }));
            setLoadedAt((current) => ({ ...current, [cacheKey]: Date.now() }));
        } catch (e) {
            setErrors((current) => ({ ...current, [cacheKey]: e instanceof Error ? e.message : String(e) }));
        } finally {
            setLoadingKey((current) => (current === cacheKey ? null : current));
        }
    }, []);

    return { models, loadedAt, loadingKey, errors, load };
}

export function ModelField({ cacheKey, value, onChange, onCommit, fallbackSuggestions, query, state, inputStyle, placeholder }: {
    cacheKey: string;
    value: string;
    onChange: (next: string) => void;
    /// The field is left — the value can be written down. A text input has no
    /// «done» of its own, and saving on every keystroke would mean a config
    /// write per letter. The value comes with it: a pick from the list commits
    /// in the same event that changed it, before React has re-rendered.
    onCommit?: (value: string) => void;
    /// The hardcoded list — all there used to be. It stays as a fallback while
    /// there is no live answer: without a key or without a network, suggesting
    /// something still beats an empty list.
    fallbackSuggestions: string[];
    query: ProviderModelsQuery;
    state: ProviderModelsState;
    inputStyle?: React.CSSProperties;
    placeholder?: string;
}) {
    const [openSignal, setOpenSignal] = useState(0);
    const fetched = state.models[cacheKey];
    const error = state.errors[cacheKey];
    const loading = state.loadingKey === cacheKey;

    // The caption is shown for `COUNT_TTL_MS` from the moment the list arrived.
    // `now` only moves when the timer fires: one re-render, at the moment the
    // line has to go.
    const loadedAt = state.loadedAt[cacheKey];
    const [now, setNow] = useState(() => Date.now());
    useEffect(() => {
        if (loadedAt === undefined) return;
        const left = COUNT_TTL_MS - (Date.now() - loadedAt);
        if (left <= 0) return;
        const timer = window.setTimeout(() => setNow(Date.now()), left);
        return () => window.clearTimeout(timer);
    }, [loadedAt]);
    const countShown = loadedAt !== undefined && now - loadedAt < COUNT_TTL_MS;
    // The current value is always in the list: otherwise an id typed by hand
    // looks like a typo next to the "correct" options.
    const suggestions = Array.from(new Set([...(fetched ?? fallbackSuggestions), value].filter(Boolean)));

    return (
        <>
            <div className="model-field" style={inputStyle}>
                <ModelCombobox
                    value={value}
                    suggestions={suggestions}
                    onChange={onChange}
                    onCommit={(next) => onCommit?.(next)}
                    placeholder={placeholder}
                    openSignal={openSignal}
                />
                {/* The app's own bubble rather than the browser's `title`: the
                    native one comes in the system font, with the system delay,
                    and looks like a stranger on the page. */}
                <Hint text={t("Запросить список моделей у провайдера")}>
                    <button
                        className="btn btn--ghost model-field__reload"
                        type="button"
                        onClick={() => { void state.load(cacheKey, query).then(() => setOpenSignal((n) => n + 1)); }}
                        disabled={loading}
                        aria-label={t("Запросить список моделей у провайдера")}
                    >
                        <Icon name="refresh" size={12}/>
                    </button>
                </Hint>
            </div>
            {/* Not a toast: "the list did not load" is not "the setting is
                broken", the model can still be typed in by hand. */}
            {error && (
                <div style={{ font: "500 10px/1.4 var(--font-sans)", color: "var(--warn)" }}>
                    {t("Список моделей: {p0}", { p0: error })}
                </div>
            )}
            {!error && fetched && countShown && (
                <div style={{ font: "500 10px/1.4 var(--font-sans)", color: "var(--ink-mute)" }}>
                    {t("вариантов от провайдера: {p0}", { p0: fetched.length })}
                </div>
            )}
        </>
    );
}
