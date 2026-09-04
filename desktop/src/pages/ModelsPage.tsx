import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "../bridge";
import { PageHeader } from "../components/Shell";
import { Icon } from "../components/Icon";
import { CustomSelect, useOutsideClose, type SelectOption } from "../components/CustomSelect";
import { useAnchoredMenu } from "../components/anchoredMenu";
import type { ConfigResult, ModelInfo } from "../bridge/types";
import { t } from "../i18n";
import { ModelActionOverlays, useModelActions } from "./modelActions";
import {
  catalogLanguages,
  EMPTY_FILTERS,
  familySections,
  fallbackModels,
  filterModels,
  languageList,
  languageSummary,
  modelMetadata,
  type CatalogFilters,
} from "./modelCatalog";

type Props = {
  models: ModelInfo[];
  config: ConfigResult | null;
  onConfigChanged: (partial: Partial<ConfigResult>) => Promise<ConfigResult | null>;
  onModelsChanged: (models: ModelInfo[]) => void;
};

export function ModelsPage({ models, config, onConfigChanged, onModelsChanged }: Props) {
  const [filters, setFilters] = useState<CatalogFilters>(EMPTY_FILTERS);
  // Свёрнутые семейства. Хранятся свёрнутые, а не развёрнутые: каталог
  // открывается целиком, и появившееся семейство не должно прятаться из-за
  // того, что его никто не разворачивал.
  const [collapsed, setCollapsed] = useState<string[]>([]);
  const visible = models.length ? models : fallbackModels();
  const selectedId = config?.model ?? visible.find((model) => model.selected)?.id ?? visible[0]?.id ?? "";
  const actions = useModelActions({
    models: visible,
    value: selectedId,
    language: config?.language,
    onConfigChanged,
    onModelsChanged,
  });

  // Каталог включает файлы, найденные в папке моделей: они могли появиться
  // или исчезнуть, пока приложение было открыто на другой вкладке.
  useEffect(() => {
    invoke<ModelInfo[]>("list_models").then(onModelsChanged).catch(() => {});
  }, [onModelsChanged]);

  // Список языков строится по самому каталогу: фильтр обязан уметь то же,
  // что умеют модели, а не то, что кто-то однажды вписал руками. Фильтр
  // спрашивает «распознает ли она мой язык», поэтому модель проходит, если
  // язык есть в её списке.
  const languageOptions = useMemo<Array<SelectOption<string>>>(
    () => [
      { value: "all", label: t("Все языки"), icon: "globe" },
      ...catalogLanguages(visible).map((language) => ({
        value: language.code,
        label: language.name,
        meta: language.code.toUpperCase(),
      })),
    ],
    [visible],
  );

  const sections = useMemo(
    () => familySections(filterModels(visible, filters)),
    [visible, filters],
  );
  const nothingFound = sections.length === 0;

  return (
    <div className="page">
      <PageHeader title={t("Модели распознавания")}/>

      <div className="model-catalog__toolbar">
        <div className="model-catalog__search">
          <Icon name="search" size={13}/>
          <input
            className="field"
            type="search"
            value={filters.query}
            placeholder={t("Поиск по названию")}
            aria-label={t("Поиск по названию")}
            onChange={(event) => setFilters((current) => ({ ...current, query: event.target.value }))}
          />
        </div>
        <CustomSelect
          value={filters.language}
          options={languageOptions}
          searchable
          inlineMeta
          onChange={(next) => setFilters((current) => ({ ...current, language: next }))}
        />
        <label className="checkbox-row">
          <input
            className="checkbox"
            type="checkbox"
            checked={filters.onlyDownloaded}
            onChange={(event) => setFilters((current) => ({ ...current, onlyDownloaded: event.target.checked }))}
          />
          {t("Только скачанные")}
        </label>
      </div>

      {nothingFound && (
        <section className="card model-catalog__empty">
          <p>{t("Ничего не нашлось. Попробуйте другое название или снимите фильтры.")}</p>
        </section>
      )}

      {sections.map((section) => {
        const isCollapsed = collapsed.includes(section.family);
        return (
          <section key={section.family} className="model-catalog__section">
            <button
              className="model-catalog__section-title"
              type="button"
              aria-expanded={!isCollapsed}
              onClick={() => setCollapsed((current) => (
                current.includes(section.family)
                  ? current.filter((family) => family !== section.family)
                  : [...current, section.family]
              ))}
            >
              <Icon name="chev-down" size={13} className={isCollapsed ? "model-catalog__chev model-catalog__chev--closed" : "model-catalog__chev"}/>
              {section.family}
            </button>
            {!isCollapsed && (
              <div className="model-grid">
                {section.models.map((model) => (
                  <ModelCard
                    key={model.id}
                    model={model}
                    active={model.id === selectedId}
                    busy={actions.isBusy(model.id)}
                    onSelect={() => actions.requestSelect(model)}
                    onDownload={() => void actions.startDownload(model)}
                    onDelete={() => actions.requestDelete(model)}
                  />
                ))}
              </div>
            )}
          </section>
        );
      })}

      <ModelActionOverlays actions={actions}/>
    </div>
  );
}

/**
 * Меню карточки.
 *
 * Удаление живёт под тремя точками, а не кнопкой в ряду: карточка целиком
 * стала кнопкой выбора, и второе действие рядом с ней должно быть заметно
 * тише первого — иначе «удалить» оказывается на расстоянии случайного клика
 * от «выбрать».
 */
function CardMenu({ busy, onDelete }: { busy: boolean; onDelete: () => void }) {
  const [open, setOpen] = useState(false);
  const anchorRef = useRef<HTMLDivElement | null>(null);
  const { menuRef, style } = useAnchoredMenu(open, anchorRef, 160, "end");
  useOutsideClose(open, anchorRef, () => setOpen(false), menuRef);
  return (
    // Клик по меню не должен доходить до карточки: она вся — кнопка выбора.
    <div ref={anchorRef} className="model-card2__menu" onClick={(event) => event.stopPropagation()}>
      <button
        className="model-card2__more"
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={t("Действия с моделью")}
        disabled={busy}
        onClick={() => setOpen((current) => !current)}
      >
        {busy ? <span className="mini-spinner"/> : <Icon name="more" size={15}/>}
      </button>
      {open && createPortal((
        <div className="custom-select__menu card-menu" role="menu" ref={menuRef} style={style}>
          <button
            className="custom-select__option card-menu__item--danger"
            type="button"
            role="menuitem"
            onClick={() => { setOpen(false); onDelete(); }}
          >
            <Icon name="trash" size={13}/>
            <span className="custom-select__text"><span className="custom-select__label">{t("Удалить")}</span></span>
          </button>
        </div>
      ), document.body)}
    </div>
  );
}

function ModelCard({ model, active, busy, onSelect, onDownload, onDelete }: {
  model: ModelInfo;
  active: boolean;
  busy: boolean;
  onSelect: () => void;
  onDownload: () => void;
  onDelete: () => void;
}) {
  const installed = model.downloaded || model.local;
  const languages = languageList(model);
  const metadata = modelMetadata(model);
  let installedClass = "";
  if (installed) installedClass = model.loaded ? "model-card2--memory" : "model-card2--installed";
  const state = [
    "model-card2",
    installedClass,
    active ? "model-card2--active" : "",
  ].filter(Boolean).join(" ");
  return (
    // Вся карточка — кнопка выбора: отдельная «Выбрать» повторяла собой то,
    // на что пользователь и так целится мышью, и отнимала строку.
    <article
      className={state}
      role="button"
      tabIndex={0}
      aria-pressed={active}
      onClick={onSelect}
      onKeyDown={(event) => {
        if (event.key !== "Enter" && event.key !== " ") return;
        event.preventDefault();
        onSelect();
      }}
    >
      <div className="model-card2__head">
        <span className="model-card2__name">{model.label}</span>
        {/* «Скачана» и «загружена» по-русски слишком похожи, чтобы различать
            ими диск и память. Про память говорим прямо. */}
        {model.loaded && (
          <span className="model-card2__state model-card2__state--memory" title={t("Модель загружена в оперативную память и распознаёт прямо сейчас.")}>
            {t("В памяти")}
          </span>
        )}
        {installed && !model.loaded && (
          <span className="model-card2__state model-card2__state--disk" title={t("Файл модели лежит на диске — интернет для неё больше не нужен.")}>
            {t("Скачана")}
          </span>
        )}
        {active && !model.loaded && (
          <span className="model-card2__state model-card2__state--disk">{t("Выбрана")}</span>
        )}
        {model.local && <span className="model-card2__state model-card2__state--own">{t("Свой файл")}</span>}
        {/* Своё удалять нечем: файл положили не мы. */}
        {installed && !model.local && <CardMenu busy={busy} onDelete={onDelete}/>}
      </div>

      {/* Свойства модели одной строкой и в одном виде: язык и потоковость —
          ответы на один и тот же вопрос «что она умеет». Не потоковая модель
          не пишет об этом ничего: «нет» здесь — это отсутствие строки, а не
          строка со словом «нет». */}
      <div className="model-card2__params">
        <span className="model-card2__param model-card2__param--wide" title={languages || undefined}>
          <Icon name="globe" size={11}/>
          {languageSummary(model)}
        </span>
        {model.streaming && (
          <span className="model-card2__param" title={t("Показывает текст по ходу диктовки, не дожидаясь конца записи.")}>
            <Icon name="spark" size={11}/>
            {t("Потоковая")}
          </span>
        )}
      </div>

      <div className="model-card2__foot">
        <span className="model-card2__meta" title={metadata}>
          {metadata}
        </span>
        {/* Скачивание — единственная кнопка в карточке: остальное делает
            клик по ней самой. Одна иконка вместо слова: подписывать
            единственное действие значит тратить строку на очевидное. */}
        {!installed && (
          <span className="model-card2__actions">
            <button
              className="btn btn--primary btn--icon btn--sm"
              type="button"
              title={t("Скачать модель")}
              aria-label={t("Скачать модель")}
              disabled={busy}
              onClick={(event) => { event.stopPropagation(); onDownload(); }}
            >
              {busy ? <span className="mini-spinner"/> : <Icon name="download" size={14}/>}
            </button>
          </span>
        )}
      </div>
    </article>
  );
}
