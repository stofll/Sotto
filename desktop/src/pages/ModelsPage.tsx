import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "../bridge";
import { PageHeader } from "../components/Shell";
import { Icon } from "../components/Icon";
import { Hint } from "../components/Hint";
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
  // Collapsed families. What is stored is the collapsed ones, not the expanded
  // ones: the catalog opens fully, and a newly appearing family must not hide
  // just because nobody has expanded it.
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

  // The catalog includes files found in the models folder: they may have
  // appeared or disappeared while the app sat on another tab.
  useEffect(() => {
    invoke<ModelInfo[]>("list_models").then(onModelsChanged).catch(() => {});
  }, [onModelsChanged]);

  // The language list is built from the catalog itself: the filter must cover
  // what the models cover, not what somebody once typed in by hand. The filter
  // asks "will it transcribe my language", so a model passes when the language
  // is in its list.
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
 * The card's menu.
 *
 * Deletion lives under the three dots rather than as a button in the row: the
 * whole card became the selection button, and a second action beside it must be
 * noticeably quieter than the first — otherwise "delete" ends up one stray click
 * away from "select".
 */
function CardMenu({ busy, onDelete }: { busy: boolean; onDelete: () => void }) {
  const [open, setOpen] = useState(false);
  const anchorRef = useRef<HTMLDivElement | null>(null);
  const { menuRef, style } = useAnchoredMenu(open, anchorRef, 160, "end");
  useOutsideClose(open, anchorRef, () => setOpen(false), menuRef);
  return (
    // A click on the menu must not reach the card: the card is all one
    // selection button.
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
  // The outline answers one question — "which of them is working right now":
  // green for the selected one, orange for one downloaded in reserve, plain for
  // one still to download. Green used to mark the model in memory while the
  // selection was not marked at all, and the selected model was
  // indistinguishable from a merely downloaded one — memory is reported
  // separately by the «В памяти» label.
  //
  // Green requires the file as well as the selection. `selectedId` is whatever
  // the config names, downloaded or not, and on a fresh install that is
  // `large-v3` with nothing on disk: without this condition the very first
  // screen marked a model as working right next to the app's own "nothing to
  // transcribe with" pill.
  const state = [
    "model-card2",
    active && installed ? "model-card2--active" : installed ? "model-card2--installed" : "",
  ].filter(Boolean).join(" ");
  return (
    // The whole card is the selection button: a separate «Выбрать» repeated
    // what the user was already aiming at with the mouse and cost a row.
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
        {/* Memory by label, disk and selection by outline: in Russian
            «скачана» and «загружена» are too alike for two adjacent labels to
            separate a file on disk from a model in RAM. */}
        {model.loaded && (
          <span className="model-card2__state model-card2__state--memory" title={t("Модель загружена в оперативную память и распознаёт прямо сейчас.")}>
            {t("В памяти")}
          </span>
        )}
        {model.local && <span className="model-card2__state model-card2__state--own">{t("Свой файл")}</span>}
        {/* A user's own file is deleted from here too. What differs is not the
            presence of a button but that the app cannot download it again —
            which is what the confirmation says. */}
        {installed && <CardMenu busy={busy} onDelete={onDelete}/>}
      </div>

      {/* The model's properties on one line and in one form: language and
          streaming answer the same question, "what can it do". A non-streaming
          model says nothing about it: "no" here is the absence of a line, not a
          line with the word "no". */}
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
        {/* Downloading is the only button on the card: everything else is done
            by clicking the card itself. One icon instead of a word: labelling
            the single action means spending a row on the obvious. */}
        {!installed && (
          <span className="model-card2__actions">
            <Hint text={t("Скачать модель")}>
              <button
                className="btn btn--primary btn--icon btn--sm"
                type="button"
                aria-label={t("Скачать модель")}
                disabled={busy}
                onClick={(event) => { event.stopPropagation(); onDownload(); }}
              >
                {busy ? <span className="mini-spinner"/> : <Icon name="download" size={14}/>}
              </button>
            </Hint>
          </span>
        )}
      </div>
    </article>
  );
}
