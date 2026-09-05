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

import { useCallback, useState } from "react";
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { Icon } from "../components/Icon";
import { t } from "../i18n";

export interface ProviderModelsQuery {
    provider: string;
    baseUrl?: string;
    apiKeyRef?: string;
}

export interface ProviderModelsState {
    /// Fetched lists by cache key (see `cacheKey` in ModelField).
    models: Record<string, string[]>;
    loadingKey: string | null;
    errors: Record<string, string>;
    load: (cacheKey: string, query: ProviderModelsQuery) => Promise<void>;
}

export function useProviderModels(): ProviderModelsState {
    const [models, setModels] = useState<Record<string, string[]>>({});
    const [loadingKey, setLoadingKey] = useState<string | null>(null);
    const [errors, setErrors] = useState<Record<string, string>>({});

    const load = useCallback(async (cacheKey: string, query: ProviderModelsQuery) => {
        setLoadingKey(cacheKey);
        setErrors((current) => {
            const next = { ...current };
            delete next[cacheKey];
            return next;
        });
        try {
            const list = await tauriInvoke<string[]>("fetch_provider_models", {
                provider: query.provider,
                base_url: query.baseUrl ?? null,
                api_key_ref: query.apiKeyRef ?? null,
            });
            setModels((current) => ({ ...current, [cacheKey]: list }));
        } catch (e) {
            setErrors((current) => ({ ...current, [cacheKey]: e instanceof Error ? e.message : String(e) }));
        } finally {
            setLoadingKey((current) => (current === cacheKey ? null : current));
        }
    }, []);

    return { models, loadingKey, errors, load };
}

export function ModelField({ cacheKey, value, onChange, fallbackSuggestions, query, state, inputStyle }: {
    cacheKey: string;
    value: string;
    onChange: (next: string) => void;
    /// The hardcoded list — all there used to be. It stays as a fallback while
    /// there is no live answer: without a key or without a network, suggesting
    /// something still beats an empty list.
    fallbackSuggestions: string[];
    query: ProviderModelsQuery;
    state: ProviderModelsState;
    inputStyle?: React.CSSProperties;
}) {
    const fetched = state.models[cacheKey];
    const error = state.errors[cacheKey];
    const loading = state.loadingKey === cacheKey;
    // The current value is always in the list: otherwise an id typed by hand
    // looks like a typo next to the "correct" options.
    const suggestions = Array.from(new Set([...(fetched ?? fallbackSuggestions), value].filter(Boolean)));
    const listId = `models-${cacheKey}`;

    return (
        <>
            <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
                <input
                    className="field mono"
                    value={value}
                    onChange={(e) => onChange(e.target.value)}
                    list={listId}
                    style={{ height: 30, fontSize: 12, ...inputStyle }}
                />
                <button
                    className="btn btn--ghost"
                    type="button"
                    onClick={() => void state.load(cacheKey, query)}
                    disabled={loading}
                    title={t("Запросить список моделей у провайдера")}
                    aria-label={t("Запросить список моделей у провайдера")}
                    style={{ height: 30, padding: "0 8px" }}
                >
                    <Icon name="refresh" size={12}/>
                </button>
            </div>
            <datalist id={listId}>
                {suggestions.map((m) => <option key={m} value={m}/>)}
            </datalist>
            {/* Not a toast: "the list did not load" is not "the setting is
                broken", the model can still be typed in by hand. */}
            {error && (
                <div style={{ font: "500 10px/1.4 var(--font-sans)", color: "var(--warn)" }}>
                    {t("Список моделей: {p0}", { p0: error })}
                </div>
            )}
            {!error && fetched && (
                <div style={{ font: "500 10px/1.4 var(--font-sans)", color: "var(--text-mute)" }}>
                    {t("вариантов от провайдера: {p0}", { p0: fetched.length })}
                </div>
            )}
        </>
    );
}
