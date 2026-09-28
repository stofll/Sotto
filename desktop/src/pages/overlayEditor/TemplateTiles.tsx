import { useState, type CSSProperties } from "react";
import { Hint } from "../../components/Hint";
import { Icon } from "../../components/Icon";
import { Modal } from "../../components/Modal";
import { RowMenu } from "../../components/RowMenu";
import { confirmDestructive } from "../../components/ConfirmDialog";
import { t } from "../../i18n";
import { OverlayScene } from "../../overlay/OverlayScene";
import { overlayPalette } from "../../overlay/overlayPalette";
import type { OverlayPreferences } from "../../overlay/overlayPreferences";
import { MAX_TEMPLATES, SHELL_LAYOUT, TEMPLATE_NAME_MAX, WINDOW_SIZE, type OverlaySize, type Recipe, type UserTemplate } from "../../overlay/overlayRecipe";
import { matchingTemplate, newTemplate, templateLook } from "./overlayDraft";
import { stillVoice } from "./simulatedVoice";
import { sameRecipe, SYSTEM_TEMPLATES, systemTemplateNames, type SystemTemplate } from "./templates";

// The box a thumbnail scene is scaled into, px.
const THUMB = { width: 108, height: 50 };

/** A still picture of an overlay, drawn by the same scene as the real window. */
export function OverlayThumb({ recipe, preferences, size }: { recipe: Recipe; preferences: OverlayPreferences; size: OverlaySize }) {
  const [width, height] = WINDOW_SIZE[SHELL_LAYOUT[recipe.shell]][size];
  const scale = Math.min(THUMB.width / width, THUMB.height / height, 1);
  return <span className="ovt" style={overlayPalette(preferences)} aria-hidden="true">
    <span className="ovt__scene" style={{ width, height, transform: `translate(-50%, -50%) scale(${scale.toFixed(3)})` } as CSSProperties}>
      <OverlayScene still recipe={recipe} size={size} phase="recording" streaming={false} draft="" draftPlaceholder=""
        timer="00:07" limited={false} status="" mode={{ full: "RU · large-v3", short: "RU" }} source={stillVoice} close={{ label: "", text: t("Отмена") }}/>
    </span>
  </span>;
}

export function SystemTemplates({ keys, recipe, preferences, onApply, disabled }: {
  keys: SystemTemplate[]; recipe: Recipe; preferences: OverlayPreferences; onApply: (key: SystemTemplate) => void; disabled?: boolean;
}) {
  const names = systemTemplateNames();
  return <div className="ovt-grid">
    {keys.map((key) => <button key={key} type="button" className="ovt-tile" disabled={disabled}
      aria-pressed={sameRecipe(SYSTEM_TEMPLATES[key], recipe)} onClick={() => onApply(key)}>
      <OverlayThumb recipe={SYSTEM_TEMPLATES[key]} preferences={preferences} size={preferences.size}/>
      <span className="ovt-tile__name">{names[key]}</span>
    </button>)}
  </div>;
}

type MineProps = {
  recipe: Recipe;
  preferences: OverlayPreferences;
  /** `strip` scrolls sideways in the overlay card; `grid` wraps in the constructor. */
  layout: "strip" | "grid";
  disabled?: boolean;
  onApply: (template: UserTemplate) => void;
  onChange: (templates: UserTemplate[], saved?: UserTemplate) => void;
  /** Opens the template in the constructor, where changes are saved back into it. */
  onEdit: (template: UserTemplate) => void;
  /** Replaces the save tile with a plain "+" that opens the constructor. */
  onCreate?: () => void;
};

/** The user's own templates: apply, save the current overlay, rename, update, delete. */
export function MyTemplates({ recipe, preferences, layout, disabled, onApply, onChange, onEdit, onCreate }: MineProps) {
  const [naming, setNaming] = useState<{ template?: UserTemplate } | null>(null);
  const templates = preferences.templates;
  const match = matchingTemplate(recipe, preferences);
  const full = templates.length >= MAX_TEMPLATES;
  const addState = match ? (match.kind === "mine" ? t("Уже сохранён") : t("Это шаблон Sotto")) : full ? t("Максимум {p0}", { p0: MAX_TEMPLATES }) : t("Сохранить текущий");
  const addHint = match ? t("Текущая настройка уже есть среди шаблонов") : full ? t("Удалите один из своих шаблонов, чтобы сохранить новый") : t("Сохранить текущий оверлей как свой шаблон");

  async function remove(template: UserTemplate) {
    if (!await confirmDestructive(t("Удалить шаблон «{p0}»? Оверлей на экране останется как есть.", { p0: template.name }))) return;
    onChange(templates.filter((item) => item.id !== template.id));
  }
  function submitName(name: string) {
    const renaming = naming?.template;
    setNaming(null);
    if (renaming) onChange(templates.map((item) => item.id === renaming.id ? { ...item, name } : item));
    else { const created = newTemplate(name, recipe, preferences); onChange([...templates, created], created); }
  }
  const defaultName = () => {
    let index = templates.length + 1;
    while (templates.some((item) => item.name === t("Мой оверлей {p0}", { p0: index }))) index++;
    return t("Мой оверлей {p0}", { p0: index });
  };

  return <div className={`ovt-mine ovt-mine--${layout}`}>
    {templates.map((template) => {
      const look = { ...preferences, ...templateLook(template) };
      const current = match?.kind === "mine" && match.template.id === template.id;
      return <div className="ovt-mine__item" key={template.id}>
        <button type="button" className="ovt-tile" disabled={disabled} aria-pressed={current} onClick={() => onApply(template)}>
          <OverlayThumb recipe={template.recipe} preferences={look} size={look.size}/>
          <span className="ovt-tile__name">{template.name}</span>
        </button>
        <span className="ovt-mine__menu">
          <RowMenu label={t("Действия с шаблоном «{p0}»", { p0: template.name })} items={[
            { id: "apply", label: t("Применить"), icon: "check", disabled: disabled || current, onSelect: () => onApply(template) },
            { id: "edit", label: t("Изменить в конструкторе"), icon: "sliders", disabled, onSelect: () => onEdit(template) },
            { id: "rename", label: t("Переименовать"), icon: "pencil", disabled, onSelect: () => setNaming({ template }) },
            { id: "delete", label: t("Удалить"), icon: "trash", danger: true, disabled, onSelect: () => void remove(template) },
          ]}/>
        </span>
      </div>;
    })}
    {onCreate
      ? <Hint text={t("Соберите оверлей в конструкторе и сохраните его, чтобы вернуться к нему одним нажатием.")}>
        <span className="ovt-mine__add">
          <button type="button" className="ovt-tile ovt-tile--new" disabled={disabled} aria-label={t("Собрать свой шаблон")} onClick={onCreate}>
            <Icon name="plus" size={20}/>
          </button>
        </span>
      </Hint>
      : <Hint text={addHint}>
        <span className="ovt-mine__add">
          <button type="button" className="ovt-tile ovt-tile--add" disabled={disabled || !!match || full} onClick={() => setNaming({})}>
            <span className="ovt-add__thumb"><OverlayThumb recipe={recipe} preferences={preferences} size={preferences.size}/><Icon name="plus" size={18}/></span>
            <span className="ovt-tile__name">{addState}</span>
          </button>
        </span>
      </Hint>}
    {!templates.length && !onCreate && <p className="ovt-mine__empty">{t("Соберите оверлей в конструкторе и сохраните его, чтобы вернуться к нему одним нажатием.")}</p>}
    {naming && <NameDialog title={naming.template ? t("Переименовать шаблон") : t("Сохранить как шаблон")}
      initial={naming.template?.name ?? defaultName()} action={naming.template ? t("Переименовать") : t("Сохранить")}
      onCancel={() => setNaming(null)} onSubmit={submitName}/>}
  </div>;
}

export function NameDialog({ title, initial, action, onSubmit, onCancel }: { title: string; initial: string; action: string; onSubmit: (name: string) => void; onCancel: () => void }) {
  const [name, setName] = useState(initial);
  const trimmed = name.trim();
  return <Modal title={title} onClose={onCancel} className="ovt-name-dialog">
    <form onSubmit={(event) => { event.preventDefault(); if (trimmed) onSubmit(trimmed); }}>
      <div className="modal__body">
        <label className="set-cell">
          <span className="set-label">{t("Название")}</span>
          <input className="field" autoFocus maxLength={TEMPLATE_NAME_MAX} value={name} onFocus={(event) => event.target.select()}
            onChange={(event) => setName(event.target.value)}/>
        </label>
      </div>
      <div className="modal__foot">
        <span/>
        <span className="ovt-name-dialog__actions">
          <button type="button" className="btn btn--ghost" onClick={onCancel}>{t("Отмена")}</button>
          <button type="submit" className="btn btn--primary" disabled={!trimmed}>{action}</button>
        </span>
      </div>
    </form>
  </Modal>;
}
