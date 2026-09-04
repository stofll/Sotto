import { useState, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Icon } from "./Icon";
import { Hint } from "./Hint";
import type { RecordingState } from "../bridge";
import { t } from "../i18n";

// Плашка статуса узкая, и вторая строка в ней делится с названием модели,
// поэтому режим подписывается коротко. Полные формулировки живут на
// странице «ИИ», где под них есть место.
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

  // Полоса делится ровно по границе сайдбара: слева она продолжает сайдбар и
  // держит название с кнопкой сворачивания, справа — фон страницы с кнопками
  // окна. Своего цвета у неё нет, поэтому «чёрной полосы» сверху больше нет.
  //
  // Перетаскивание окна держится на `data-tauri-drag-region`, а не на
  // `-webkit-app-region: drag`: последнее понимает только WebView2, поэтому на
  // macOS (WKWebView) окно меняло размер, но не двигалось. Значение `deep`
  // распространяет зону на всю полосу; кнопки Tauri исключает сам — любой
  // `<button>` на пути события отменяет перетаскивание.
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

// Название живёт в рельсе титулбара, а не в самом сайдбаре: строка с
// кнопками окна всё равно занимает верхние 38 px, и держать под ней ещё одну
// строку с именем значило дважды тратить высоту. Разделы теперь начинаются от
// самого верха сайдбара.
export function Brand({ collapsed, onToggleCollapse }: { collapsed?: boolean; onToggleCollapse?: () => void }) {
  // Кнопка одна и та же в обоих состояниях — меняется только место: справа от
  // названия, а в свёрнутом сайдбаре она единственная и стоит по центру.
  // Прятать её в наведение нельзя: на элемент, о котором не знаешь, не
  // наводят — это та же непрочитанная кнопка, что и раньше, только без
  // собственного места.
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

export function StatusPill({ state, pipelineMode, loadedModel, downloadProgress, compact }: { state?: RecordingState; pipelineMode?: string; loadedModel?: string | null; downloadProgress?: DownloadProgress | null; compact?: boolean }) {
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
  // Без модели «Готово» — ложь: писать можно, но распознать нечем. В покое
  // отсутствие модели само становится заголовком, а вторая строка остаётся
  // под режим конвейера, чтобы подпись не дублировала её.
  const idleWithoutModel = !model && !active && current !== "error";
  const headline = idleWithoutModel ? t("Модель не загружена") : s.text;
  const dot = idleWithoutModel ? "var(--text-mute)" : s.dot;
  const modelLabel = model || t("Модель не загружена");
  const detail = idleWithoutModel
    ? (pipelineLabel || "")
    : (pipelineLabel ? `${modelLabel} · ${pipelineLabel}` : modelLabel);

  // Свёрнутый сайдбар получает отдельную разметку, а не ту же с погашенными
  // детьми: отступы и flex у плашки заданы инлайном, и CSS свёрнутого
  // состояния их не перебивал — коробка оставалась «широкой», а точка в ней
  // уезжала от центра на ширину скрытого текста.
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

// Функция, а не константа: подписи переводятся, и вычисленные при импорте
// они застряли бы на языке по умолчанию. Значения цветов при этом остаются
// литеральными типами, поэтому AccentValue по-прежнему объединение хексов.
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

export function Sidebar({ tab, onTab, recordingState, pipelineMode, loadedModel, theme, onToggleTheme, downloadProgress, collapsed: sidebarCollapsed }: { tab: TabId; onTab: (tab: TabId) => void; recordingState?: RecordingState; pipelineMode?: string; loadedModel?: string | null; theme: "dark" | "light"; onToggleTheme: () => void; downloadProgress?: DownloadProgress | null; collapsed?: boolean }) {
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
      <div className="sidebar-footer"><StatusPill state={recordingState} pipelineMode={pipelineMode} loadedModel={loadedModel} downloadProgress={downloadProgress} compact={sidebarCollapsed}/></div>
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

export function Segmented({ value, options, onChange, disabled = false }: { value: string; options: Array<string | { value: string; label: string; icon?: string }>; onChange?: (value: string) => void; disabled?: boolean }) {
  return (
    <div style={{ display: "inline-flex", padding: 3, gap: 2, background: "var(--surface-2)", border: "1px solid var(--border-strong)", borderRadius: "var(--r-sm)", opacity: disabled ? 0.55 : 1 }}>
      {options.map((opt) => {
        const v = typeof opt === "string" ? opt : opt.value;
        const label = typeof opt === "string" ? opt : opt.label;
        const icon = typeof opt === "string" ? undefined : opt.icon;
        const selected = value === v;
        return <button key={v} type="button" disabled={disabled} onClick={() => onChange?.(v)} style={{ appearance: "none", display: "inline-flex", alignItems: "center", gap: 6, height: 26, padding: "0 12px", border: 0, cursor: disabled ? "not-allowed" : "pointer", borderRadius: 4, background: selected ? "var(--surface-4)" : "transparent", color: selected ? "var(--text)" : "var(--text-2)", font: "500 12px/1 var(--font-sans)", boxShadow: selected ? "0 1px 0 rgba(255,255,255,0.04) inset, 0 1px 2px rgba(0,0,0,0.2)" : "none" }}>{icon && <Icon name={icon} size={13}/>} {label}</button>;
      })}
    </div>
  );
}

export function Bars({ value = 0.6, color = "var(--accent)", segments = 24 }: { value?: number; color?: string; segments?: number }) {
  return <div style={{ display: "flex", gap: 2, height: 14, alignItems: "center" }}>{Array.from({ length: segments }).map((_, i) => <span key={i} style={{ width: 3, height: i / segments < value ? 4 + (i % 5) * 2 : 4, background: i / segments < value ? color : "var(--surface-3)", borderRadius: 1 }}/>)}</div>;
}

export function Waveform({ bars = 28, color = "var(--accent)" }: { bars?: number; color?: string }) {
  return <div style={{ display: "flex", alignItems: "center", gap: 3, height: 24 }}>{Array.from({ length: bars }).map((_, i) => <span key={i} style={{ width: 3, height: "100%", background: color, borderRadius: 2, transformOrigin: "center", animation: `wave-pulse ${0.6 + (i % 5) * 0.1}s ease-in-out ${(i * 0.05) % 1}s infinite` }}/>)}</div>;
}
