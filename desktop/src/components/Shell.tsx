import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Icon } from "./Icon";
import { Hint } from "./Hint";
import type { RecordingState } from "../bridge";
import { t } from "../i18n";

// The status pill is narrow and its second line is shared with the model name,
// so the mode gets a short caption. The full wording lives on the «ИИ» page,
// where there is room for it.
const PIPELINE_LABEL = (): Record<string, string> => ({
  local: t("Локально"),
  hybrid: t("Локально + LLM"),
  cloud: t("Облако"),
});

export type TabId = "settings" | "models" | "text" | "ai" | "integrations" | "history" | "stats" | "info";

type NavItem = { id: TabId; label: string; icon: string; count?: number };
type NavGroup = { id: string; label: string; items: NavItem[] };

export const NAV_GROUPS = (): NavGroup[] => ([
  { id: "core", label: t("Основное"), items: [
    { id: "settings", label: t("Настройки"), icon: "sliders" },
    { id: "models", label: t("Модели"), icon: "cpu" },
  ] },
  { id: "processing", label: t("Обработка"), items: [
    { id: "text", label: t("Текст"), icon: "text" },
    { id: "ai", label: t("LLM-обработка"), icon: "spark" },
  ] },
  { id: "integrations", label: t("Интеграции"), items: [
    { id: "integrations", label: t("Провайдеры и ключи"), icon: "server" },
  ] },
  { id: "data", label: t("Данные"), items: [
    { id: "history", label: t("История"), icon: "clock" },
    { id: "stats", label: t("Статистика"), icon: "chart" },
  ] },
  { id: "help", label: t("Помощь"), items: [{ id: "info", label: t("Справка"), icon: "info" }] },
]);

export function TitleBar({ collapsed, onToggleCollapse }: { collapsed?: boolean; onToggleCollapse?: () => void }) {
  async function withWindow(action: "minimize" | "maximize" | "close") {
    const win = getCurrentWindow();
    if (action === "minimize") await win.minimize();
    if (action === "maximize") await win.toggleMaximize();
    if (action === "close") await win.close();
  }

  // The bar is split exactly along the sidebar edge: on the left it continues
  // the sidebar and carries the name with the collapse button, on the right it
  // is the page background with the window buttons. It has no colour of its own,
  // so there is no longer a "black stripe" across the top.
  //
  // Window dragging rests on `data-tauri-drag-region` rather than on
  // `-webkit-app-region: drag`: only WebView2 understands the latter, so on
  // macOS (WKWebView) the window resized but would not move. The `deep` value
  // spreads the zone across the whole bar; Tauri excludes buttons itself — any
  // `<button>` in the event path cancels the drag.
  return (
    <div className="titlebar" data-tauri-drag-region="deep">
      <div className="titlebar__rail"><Brand collapsed={collapsed} onToggleCollapse={onToggleCollapse}/></div>
      <div className="titlebar__bar">
        <button className="btn btn--ghost titlebar__button" onClick={() => void withWindow("minimize")} aria-label={t("Свернуть")}><svg width="10" height="10" viewBox="0 0 10 10"><path d="M2 5h6" stroke="currentColor" strokeWidth="1"/></svg></button>
        <button className="btn btn--ghost titlebar__button" onClick={() => void withWindow("maximize")} aria-label={t("Развернуть")}><svg width="10" height="10" viewBox="0 0 10 10"><rect x="2" y="2" width="6" height="6" stroke="currentColor" strokeWidth="1" fill="none"/></svg></button>
        <button className="btn btn--ghost titlebar__button titlebar__button--close" onClick={() => void withWindow("close")} aria-label={t("Закрыть")}><svg width="10" height="10" viewBox="0 0 10 10"><path d="M2 2l6 6M8 2l-6 6" stroke="currentColor" strokeWidth="1"/></svg></button>
      </div>
    </div>
  );
}

// The name lives in the title-bar rail rather than in the sidebar itself: the
// row with the window buttons occupies the top 38 px anyway, and keeping another
// row with the name below it meant spending that height twice. Sections now
// start at the very top of the sidebar.
export function Brand({ collapsed, onToggleCollapse }: { collapsed?: boolean; onToggleCollapse?: () => void }) {
  // The button is the same in both states — only its place changes: to the right
  // of the name, and in a collapsed sidebar it is the only one and sits centred.
  // Hiding it behind hover is not an option: nobody hovers over an element they
  // do not know about — that is the same unread button as before, only without a
  // place of its own.
  const label = collapsed ? t("Развернуть сайдбар") : t("Свернуть сайдбар");
  return (
    <div className="sidebar-brand">
      <div className="sidebar-brand__name">Sotto</div>
      {onToggleCollapse && (
        <button
          className="sidebar-brand__toggle"
          type="button"
          onClick={onToggleCollapse}
          aria-label={label}
          title={label}
        >
          <Icon name="panel" size={15}/>
        </button>
      )}
    </div>
  );
}

export type DownloadProgress = { model?: string; downloaded: number; total: number | null };

function formatBytes(bytes: number): string {
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(0)} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

export function StatusPill({ state, pipelineMode, loadedModel, loadsOnDemand, downloadProgress, compact }: { state?: RecordingState; pipelineMode?: string; loadedModel?: string | null; loadsOnDemand?: boolean; downloadProgress?: DownloadProgress | null; compact?: boolean }) {
  const map: Record<RecordingState, { dot: string; text: string; sub: string }> = {
    idle: { dot: "var(--ok)", text: t("Готово"), sub: "" },
    recording: { dot: "var(--rec)", text: t("Идёт запись"), sub: "" },
    processing: { dot: "var(--accent)", text: t("Распознаю"), sub: "" },
    done: { dot: "var(--ok)", text: t("Готово"), sub: "" },
    loading: { dot: "var(--accent)", text: t("Загружаю модель"), sub: "..." },
    error: { dot: "var(--err)", text: t("Ошибка"), sub: "" },
  };
  const current = state || "idle";
  const s = map[current];
  const model = loadedModel?.trim() || "";
  const pipelineLabel = PIPELINE_LABEL()[pipelineMode ?? ""];
  const active = current !== "idle" && current !== "done" && current !== "error";
  const isCloud = pipelineMode === "cloud";
  // With no model «Готово» is a lie: you can record, but there is nothing to
  // transcribe with. At rest the absence of a model becomes the headline itself,
  // while the second line stays reserved for the pipeline mode so the caption
  // does not duplicate it.
  const idleWithoutModel = !model && !active && current !== "error";
  // A model unloaded on idle is not a loss: the file is in place and the very
  // next dictation brings it back into memory. «Не загружена» would read here as
  // a breakage and would send a person off to repair something that works.
  const headline = idleWithoutModel
    ? (loadsOnDemand ? t("Модель выгружена") : t("Модель не загружена"))
    : s.text;
  const dot = idleWithoutModel ? "var(--text-mute)" : s.dot;
  const modelLabel = model || t("Модель не загружена");
  const idleDetail = [loadsOnDemand ? t("вернётся при диктовке") : "", pipelineLabel]
    .filter(Boolean)
    .join(" · ");
  const detail = idleWithoutModel
    ? idleDetail
    : (pipelineLabel ? `${modelLabel} · ${pipelineLabel}` : modelLabel);

  // A collapsed sidebar gets its own markup rather than the same markup with
  // children hidden: the pill's padding and flex are set inline, and the CSS for
  // the collapsed state could not override them — the box stayed "wide" and the
  // dot inside drifted off centre by the width of the hidden text.
  if (compact) {
    const title = detail ? `${headline} · ${detail}` : headline;
    return (
      <div title={title} style={{ display: "grid", placeItems: "center", height: 42, borderRadius: "var(--r)", background: "var(--surface-2)", border: isCloud ? "1px solid var(--border-accent)" : "1px solid var(--border)" }}>
        <span style={{ width: 8, height: 8, borderRadius: "50%", background: dot, boxShadow: active ? `0 0 0 4px ${dot}22` : "none", animation: active ? "rec-halo 1.4s ease-out infinite" : "none" }}/>
      </div>
    );
  }

  const showDownload = current === "loading" && downloadProgress && downloadProgress.downloaded > 0;
  if (showDownload) {
    const { downloaded, total, model } = downloadProgress;
    const percent = total && total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : null;
    const headline = model ? t("Скачиваю {p0}", { p0: model }) : t("Скачиваю модель");
    const detail = total
      ? `${formatBytes(downloaded)} / ${formatBytes(total)}${percent != null ? ` · ${percent}%` : ""}`
      : t("{p0} (размер уточняется…)", { p0: formatBytes(downloaded) });
    return (
      <div style={{ display: "flex", flexDirection: "column", gap: 6, padding: "10px 12px", background: "var(--surface-2)", border: "1px solid var(--border)", borderRadius: "var(--r)" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span style={{ width: 8, height: 8, borderRadius: "50%", background: s.dot, boxShadow: `0 0 0 4px ${s.dot}22`, animation: "rec-halo 1.4s ease-out infinite", flex: "0 0 auto" }}/>
          <div style={{ minWidth: 0, flex: 1 }}>
            <div style={{ font: "500 12px/1.1 var(--font-sans)", color: "var(--text)" }}>{headline}</div>
            <div style={{ font: "500 10px/1 var(--font-mono)", color: "var(--text-mute)", marginTop: 3, letterSpacing: "0.04em" }}>{detail}</div>
          </div>
        </div>
        <div style={{ position: "relative", height: 4, borderRadius: 999, background: "var(--surface-3)", overflow: "hidden" }}>
          {percent != null
            ? <div style={{ position: "absolute", inset: 0, width: `${percent}%`, background: "var(--accent)", borderRadius: 999, transition: "width 200ms ease" }}/>
            : <div style={{ position: "absolute", inset: 0, width: "40%", background: "linear-gradient(90deg, transparent, var(--accent), transparent)", animation: "progress-sweep 1.15s ease-in-out infinite", borderRadius: 999 }}/>
          }
        </div>
      </div>
    );
  }

  return (
    <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "10px 12px", background: "var(--surface-2)", border: isCloud ? "1px solid var(--border-accent)" : "1px solid var(--border)", borderRadius: "var(--r)" }}>
      <span style={{ width: 8, height: 8, borderRadius: "50%", background: dot, boxShadow: active ? `0 0 0 4px ${dot}22` : "none", animation: active ? "rec-halo 1.4s ease-out infinite" : "none", flex: "0 0 auto" }}/>
      <div className="status-pill__text" style={{ minWidth: 0, flex: 1 }}>
        <div style={{ font: "500 12px/1.1 var(--font-sans)", color: "var(--text)", display: "flex", alignItems: "center", gap: 6 }}>{headline}{isCloud && <span className="tag tag--rec" style={{ height: 16, fontSize: 9, padding: "0 5px" }}>Cloud</span>}</div>
        {detail && <div style={{ font: "500 10px/1.2 var(--font-mono)", color: "var(--text-mute)", marginTop: 3, letterSpacing: "0.04em", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }} title={detail}>{detail}</div>}
      </div>
    </div>
  );
}

// A function rather than a constant: the labels are translated, and computed at
// import time they would be stuck in the default language. The colour values
// stay literal types, so AccentValue is still a union of hex strings.
export const ACCENT_OPTIONS = () => ([
  { value: "#e68a3d", strong: "#f5993f", ink: "#1a1208", label: t("Оранжевый") },
  { value: "#5b8def", strong: "#6f9bf3", ink: "#091226", label: t("Синий") },
  { value: "#3dc97c", strong: "#4ed688", ink: "#082416", label: t("Зелёный") },
  { value: "#9b75ef", strong: "#a886f3", ink: "#180a2c", label: t("Фиолетовый") },
] as const);
export type AccentValue = ReturnType<typeof ACCENT_OPTIONS>[number]["value"];

export function applyAccent(hex: string) {
  const options = ACCENT_OPTIONS();
  const opt = options.find((o) => o.value.toLowerCase() === hex.toLowerCase()) ?? options[0];
  const root = document.documentElement;
  root.style.setProperty("--accent", opt.value);
  root.style.setProperty("--accent-strong", opt.strong);
  root.style.setProperty("--accent-ink", opt.ink);
  const m = opt.value.match(/^#(.{2})(.{2})(.{2})$/);
  if (m) {
    const r = parseInt(m[1], 16), g = parseInt(m[2], 16), b = parseInt(m[3], 16);
    root.style.setProperty("--accent-soft", `rgba(${r}, ${g}, ${b}, 0.14)`);
    root.style.setProperty("--accent-soft-2", `rgba(${r}, ${g}, ${b}, 0.26)`);
    root.style.setProperty("--accent-line", `rgba(${r}, ${g}, ${b}, 0.32)`);
  }
}

export function Sidebar({ tab, onTab, recordingState, pipelineMode, loadedModel, loadsOnDemand, theme, onToggleTheme, downloadProgress, collapsed: sidebarCollapsed }: { tab: TabId; onTab: (tab: TabId) => void; recordingState?: RecordingState; pipelineMode?: string; loadedModel?: string | null; loadsOnDemand?: boolean; theme: "dark" | "light"; onToggleTheme: () => void; downloadProgress?: DownloadProgress | null; collapsed?: boolean }) {
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>(() => {
    try {
      const saved = window.localStorage.getItem("sotto.nav.collapsed");
      if (saved) return JSON.parse(saved) as Record<string, boolean>;
    } catch {/* ignore */}
    return { data: true, help: true };
  });

  function toggleGroup(id: string) {
    setCollapsed((current) => {
      const next = { ...current, [id]: !current[id] };
      try { window.localStorage.setItem("sotto.nav.collapsed", JSON.stringify(next)); } catch {/* ignore */}
      return next;
    });
  }

  return (
    <aside className="win__sidebar">
      <nav className="nav" aria-label={t("Разделы")}>
        {NAV_GROUPS().map((group) => {
          const active = group.items.some((item) => item.id === tab);
          const isCollapsed = collapsed[group.id] && !active;
          return (
            <div key={group.id} className="nav__group-wrap">
              <button className="nav__group" type="button" aria-expanded={!isCollapsed} onClick={() => toggleGroup(group.id)}>
                <span>{group.label}</span>
                <Icon name="chev-down" size={12} style={{ transform: isCollapsed ? "rotate(-90deg)" : "none", transition: "transform 160ms ease" }}/>
              </button>
              <div className="nav__group-items" data-collapsed={isCollapsed ? "true" : "false"}>
                <div>
                  {group.items.map((item) => (
                    <button key={item.id} className="nav__item" aria-selected={tab === item.id} onClick={() => onTab(item.id)} title={sidebarCollapsed ? item.label : undefined}>
                      <span style={{ color: tab === item.id ? "var(--accent)" : "var(--text-mute)", display: "flex" }}><Icon name={item.icon} size={15}/></span>
                      <span>{item.label}</span>
                      {item.count != null && <span className="nav__count">{item.count}</span>}
                    </button>
                  ))}
                </div>
              </div>
            </div>
          );
        })}
      </nav>
      <div className="sidebar-actions">
        <button className="theme-toggle" type="button" onClick={onToggleTheme} aria-label={theme === "dark" ? t("Включить светлую тему") : t("Включить темную тему")}>
          <span className="theme-toggle__icon"><Icon name={theme === "dark" ? "moon" : "sun"} size={15}/></span>
          <span className="theme-toggle__text">{t("Тема")}</span>
          <span className="theme-toggle__value">{theme === "dark" ? t("Темная") : t("Светлая")}</span>
        </button>
      </div>
      <div className="sidebar-footer"><StatusPill state={recordingState} pipelineMode={pipelineMode} loadedModel={loadedModel} loadsOnDemand={loadsOnDemand} downloadProgress={downloadProgress} compact={sidebarCollapsed}/></div>
    </aside>
  );
}

export function MainHeader({ title, subtitle, right, className, titleExtra, breadcrumb }: { title: string; subtitle?: string; right?: ReactNode; className?: string; titleExtra?: ReactNode; breadcrumb?: string }) {
  return <header className={["main-header", className].filter(Boolean).join(" ")}><div>{breadcrumb && <div style={{ marginBottom: 6, font: "500 10px/1 var(--font-mono)", color: "var(--text-mute)", letterSpacing: "0.08em", textTransform: "uppercase" }}>{breadcrumb}</div>}<h1>{title}{titleExtra}</h1>{subtitle && <p>{subtitle}</p>}</div>{right && <div>{right}</div>}</header>;
}

/** Redesigned page header used by Stage-2+ pages. Renders inside `.page`. */
export function PageHeader({ title, sub, actions }: { title: string; sub?: string; actions?: ReactNode }) {
  return (
    <div className="page-header">
      <div className="page-header__main">
        <h1 className="page-title">{title}</h1>
        {sub && <p className="page-sub">{sub}</p>}
      </div>
      {actions && <div className="page-actions">{actions}</div>}
    </div>
  );
}

/** Small uppercase mono section label used inside redesigned pages. */
export function SectionLabel({ children }: { children: ReactNode }) {
  return <div className="section-label">{children}</div>;
}

export function SettingRow({ title, hint, stack, children }: { title: string; hint?: string; stack?: boolean; children: ReactNode }) {
  return (
    <div className={stack ? "setting setting--stack" : "setting"}>
      <div className="setting__label">
        <h3>
          {title}
          {hint && <Hint text={hint}/>}
        </h3>
      </div>
      <div className="setting__control">{children}</div>
    </div>
  );
}

export function Switch({ on, onChange }: { on: boolean; onChange?: (value: boolean) => void }) {
  return <button className="switch" data-on={on ? "true" : "false"} onClick={() => onChange?.(!on)} aria-pressed={on} aria-label={on ? t("Включено") : t("Выключено")}/>;
}

/**
 * A switch made of several segments.
 *
 * The selection highlight is not the button's own background but a separate
 * backdrop that slides beneath it. Selection used to repaint two buttons
 * instantly, and on long captions («Переключатель» → «Удержание») that read as
 * one element being swapped for another rather than as movement. The backdrop
 * can only slide if it knows where to — hence measuring the selected button
 * instead of computing a fraction of the width: segments come in different
 * widths, and between Russian and English those widths differ as well.
 *
 * The styles moved into CSS: neither the transition nor respect for
 * `prefers-reduced-motion`.
 */
export function Segmented({ value, options, onChange, disabled = false }: { value: string; options: Array<string | { value: string; label: string; icon?: string }>; onChange?: (value: string) => void; disabled?: boolean }) {
  const items = options.map((opt) => (typeof opt === "string" ? { value: opt, label: opt, icon: undefined } : opt));
  const rootRef = useRef<HTMLDivElement>(null);
  const [thumb, setThumb] = useState<{ left: number; width: number } | null>(null);
  const selectedIndex = items.findIndex((item) => item.value === value);
  // The captions are part of the key on purpose: changing the UI language
  // changes the button widths without touching the selection or their count.
  const labelsKey = items.map((item) => item.label).join("\u0000");

  useLayoutEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    const button = root.querySelectorAll<HTMLButtonElement>(".segmented__option")[selectedIndex];
    if (!button) {
      setThumb(null);
      return;
    }
    // Both share one origin — the switch's padding-box: `offsetLeft` is measured
    // from it, and so is the backdrop's `left: 0`. The border-width correction
    // that suggests itself shifts the backdrop by exactly that one pixel to the
    // left — verified by measurement.
    const measure = () => setThumb({ left: button.offsetLeft, width: button.offsetWidth });
    measure();
    // Segment width depends on the width of the row: in a narrow window «Режим
    // записи» shrinks along with its column, and the backdrop must follow it.
    const observer = new ResizeObserver(measure);
    observer.observe(root);
    return () => observer.disconnect();
  }, [selectedIndex, items.length, labelsKey]);

  return (
    <div className="segmented" ref={rootRef} data-disabled={disabled ? "true" : undefined}>
      {/* It appears after the first measurement, so there is no slide-in from
          the left on mount: the element is born already in place, and the
          transition starts working from the next change of selection. */}
      {thumb && <span className="segmented__thumb" aria-hidden="true" style={{ transform: `translateX(${thumb.left}px)`, width: thumb.width }}/>}
      {items.map((item) => (
        <button
          key={item.value}
          type="button"
          className="segmented__option"
          data-selected={item.value === value ? "true" : "false"}
          disabled={disabled}
          onClick={() => onChange?.(item.value)}
        >
          {item.icon && <Icon name={item.icon} size={13}/>} {item.label}
        </button>
      ))}
    </div>
  );
}

export function Bars({ value = 0.6, color = "var(--accent)", segments = 24 }: { value?: number; color?: string; segments?: number }) {
  return <div style={{ display: "flex", gap: 2, height: 14, alignItems: "center" }}>{Array.from({ length: segments }).map((_, i) => <span key={i} style={{ width: 3, height: i / segments < value ? 4 + (i % 5) * 2 : 4, background: i / segments < value ? color : "var(--surface-3)", borderRadius: 1 }}/>)}</div>;
}

export function Waveform({ bars = 28, color = "var(--accent)" }: { bars?: number; color?: string }) {
  return <div style={{ display: "flex", alignItems: "center", gap: 3, height: 24 }}>{Array.from({ length: bars }).map((_, i) => <span key={i} style={{ width: 3, height: "100%", background: color, borderRadius: 2, transformOrigin: "center", animation: `wave-pulse ${0.6 + (i % 5) * 0.1}s ease-in-out ${(i * 0.05) % 1}s infinite` }}/>)}</div>;
}
