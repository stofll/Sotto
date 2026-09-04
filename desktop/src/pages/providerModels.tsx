// Поле «Model» со списком, который провайдер отдаёт сам.
//
// Раньше id вводился руками, а подсказка отсылала читать документацию.
// Провайдеры переименовывают и снимают модели чаще, чем выходят релизы
// приложения, поэтому зашитый в код список устаревает молча — и узнаёшь об
// этом в момент диктовки, когда разбираться меньше всего хочется.
//
// Запрос уходит только по явному действию: при смене провайдера и по кнопке
// обновления. Никакого фонового опроса — это сетевой запрос с ключом
// пользователя. Поле остаётся свободным для ввода: раньше вписанные вручную
// id обязаны продолжать работать, даже если провайдер их больше не
// перечисляет.

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
    /// Подтянутые списки по ключу кэша (см. `cacheKey` в ModelField).
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
    /// Зашитый список — им и ограничивались раньше. Остаётся как запасной
    /// вариант, пока живого ответа нет: без ключа или без сети подсказать
    /// что-то всё равно лучше, чем пустой список.
    fallbackSuggestions: string[];
    query: ProviderModelsQuery;
    state: ProviderModelsState;
    inputStyle?: React.CSSProperties;
}) {
    const fetched = state.models[cacheKey];
    const error = state.errors[cacheKey];
    const loading = state.loadingKey === cacheKey;
    // Текущее значение всегда в списке: иначе вручную вписанный id выглядит
    // как опечатка рядом с «правильными» вариантами.
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
            {/* Не тост: «список не подтянулся» — это не «настройка сломана»,
                модель по-прежнему можно вписать руками. */}
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
