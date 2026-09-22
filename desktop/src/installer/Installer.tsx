import { useEffect, useRef, useState } from "react";
import { chooseSetupDirectory, closeSetup, installSotto, launchSotto, newestSetupStatus, onSetupStatus, setupOptions, setupStatus, type SetupInstallOptions, type SetupStatus } from "../bridge/installer";
import { setLocale, t, useLocale } from "../i18n";
import { Icon } from "../components/Icon";
import { Hint } from "../components/Hint";
import { useOutsideClose } from "../components/CustomSelect";

const native = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

function errorText(code: string | null): string {
  switch (code) {
    case "payload_missing": return t("В этой сборке нет установочного пакета.");
    case "payload_invalid": return t("Установочный пакет повреждён. Скачайте установщик заново.");
    case "prepare_failed": return t("Не удалось подготовить файлы. Проверьте свободное место и повторите попытку.");
    case "install_failed": return t("Установка не завершена. Закройте Sotto и повторите попытку.");
    case "launch_failed": return t("Не удалось запустить Sotto. Откройте приложение через меню «Пуск».");
    case "preview_only": return t("Это демонстрация интерфейса. Файлы приложения не изменяются.");
    case "invalid_install_directory": return t("Укажите полный путь к папке, например D:\\Apps\\Sotto.");
    case "install_directory_not_empty": return t("Выберите пустую папку для установки Sotto.");
    case "install_directory_locked": return t("При обновлении используется папка установленной Sotto.");
    case "options_unavailable": return t("Не удалось открыть выбор папки. Введите путь вручную.");
    default: return t("Не удалось связаться с установщиком. Закройте это окно и откройте установщик снова.");
  }
}

export function Installer() {
  const locale = useLocale();
  const [status, setStatus] = useState<SetupStatus | null>(native ? null : {
    phase: "ready", revision: 0, version: "", preview: true, error: null,
  });
  const [connectionError, setConnectionError] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [options, setOptions] = useState<SetupInstallOptions | null>(native ? null : {
    install_dir: "C:\\Users\\Demo\\AppData\\Local\\Sotto", desktop_shortcut: true, start_menu_shortcut: true,
  });
  const [directoryLocked, setDirectoryLocked] = useState(false);
  const [screen, setScreen] = useState<"welcome" | "options">("welcome");
  const [choosingDirectory, setChoosingDirectory] = useState(false);
  const [optionsError, setOptionsError] = useState<string | null>(null);
  const optionsHeading = useRef<HTMLHeadingElement>(null);
  const optionsTrigger = useRef<HTMLButtonElement>(null);
  const directoryInput = useRef<HTMLInputElement>(null);
  const previousScreen = useRef(screen);
  const [details, setDetails] = useState(false);
  const [theme, setTheme] = useState(() => matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light");
  const [visible, setVisible] = useState(!document.hidden);
  const timers = useRef<ReturnType<typeof setTimeout>[]>([]);
  const actionInFlight = useRef(false);
  const heading = useRef<HTMLHeadingElement>(null);
  const about = useRef<HTMLDivElement>(null);
  const aboutButton = useRef<HTMLButtonElement>(null);
  useOutsideClose(details, about, () => {
    setDetails(false);
    if (about.current?.contains(document.activeElement)) aboutButton.current?.focus();
  });
  const phase = status?.phase ?? "ready";
  const busy = pending || phase === "preparing" || phase === "installing";
  const preview = status?.preview ?? false;
  const showingOptions = screen === "options" && !busy && phase !== "complete";

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);
  useEffect(() => {
    const listener = () => setVisible(!document.hidden);
    document.addEventListener("visibilitychange", listener);
    return () => document.removeEventListener("visibilitychange", listener);
  }, []);
  useEffect(() => () => timers.current.forEach(clearTimeout), []);
  useEffect(() => {
    if (!native) return;
    let disposed = false;
    let stop: (() => void) | undefined;
    const update = (next: SetupStatus) => {
      if (!disposed) setStatus((current) => newestSetupStatus(current, next));
    };
    void (async () => {
      try {
        stop = await onSetupStatus(update);
        if (disposed) { stop(); return; }
        const [snapshot, defaults] = await Promise.all([setupStatus(), setupOptions()]);
        update(snapshot);
        if (!disposed) { setOptions(defaults.options); setDirectoryLocked(defaults.directory_locked); }
      } catch {
        if (!disposed) setConnectionError(true);
      }
    })();
    return () => { disposed = true; stop?.(); };
  }, []);
  useEffect(() => {
    if (phase === "complete") heading.current?.focus();
    if (phase === "failed") (showingOptions ? optionsHeading : heading).current?.focus();
  }, [phase, showingOptions]);
  useEffect(() => {
    if (screen === previousScreen.current) return;
    previousScreen.current = screen;
    if (screen === "options") optionsHeading.current?.focus();
    else optionsTrigger.current?.focus();
  }, [screen]);

  function openOptions() { setDetails(false); setScreen("options"); }

  async function browse() {
    if (!options || choosingDirectory) return;
    if (!native) { directoryInput.current?.focus(); directoryInput.current?.select(); return; }
    setChoosingDirectory(true);
    setOptionsError(null);
    try {
      const directory = await chooseSetupDirectory(options.install_dir);
      if (directory) setOptions(current => current && ({ ...current, install_dir: directory }));
    } catch { setOptionsError("options_unavailable"); }
    finally { setChoosingDirectory(false); }
  }

  async function start() {
    if (actionInFlight.current || busy || choosingDirectory || !options) return;
    if (!/^[a-zA-Z]:[\\/].+/.test(options.install_dir) || /["<>|?*\u0000-\u001f]/.test(options.install_dir)) {
      setOptionsError("invalid_install_directory"); openOptions(); directoryInput.current?.focus(); return;
    }
    setOptionsError(null);
    actionInFlight.current = true;
    setPending(true);
    setActionError(null);
    if (preview) {
      const next = (phase: SetupStatus["phase"], error: string | null = null) =>
        setStatus((s) => s && ({ ...s, phase, error, revision: s.revision + 1 }));
      next("preparing");
      timers.current.push(setTimeout(() => next("installing"), 800));
      timers.current.push(setTimeout(() => {
        const fail = new URLSearchParams(location.search).get("demo") === "error";
        next(fail ? "failed" : "complete", fail ? "install_failed" : null);
        setPending(false);
        actionInFlight.current = false;
      }, 3600));
      return;
    }
    try { await installSotto(options); }
    catch (error) { setActionError(String(error).includes("payload_missing") ? "payload_missing" : "install_failed"); }
    finally { setPending(false); actionInFlight.current = false; }
  }

  async function launch() {
    if (preview) { setActionError("preview_only"); return; }
    setPending(true);
    try { await launchSotto(); }
    catch { setActionError("launch_failed"); }
    finally { setPending(false); }
  }

  return <main className="app-frame setup-window" data-moving={visible && phase !== "failed"} data-busy={busy}>
    <div className="setup-waves" aria-hidden="true">
      <svg viewBox="0 0 1440 600" preserveAspectRatio="none">
        <path className="setup-wave setup-wave-back" d="M-720 300 C-360 100 0 500 360 300 S1080 100 1440 300 S2160 500 2520 300 L2520 650 H-720Z" />
        <path className="setup-wave setup-wave-mid" d="M-720 360 C-360 180 0 540 360 360 S1080 180 1440 360 S2160 540 2520 360 L2520 650 H-720Z" />
        <path className="setup-wave setup-wave-front" d="M-720 430 C-360 270 0 590 360 430 S1080 270 1440 430 S2160 590 2520 430 L2520 650 H-720Z" />
      </svg>
    </div>
    <header className="setup-toolbar">
      <div className="setup-tools">
        <button className="btn btn--ghost" onClick={() => setLocale(locale === "ru" ? "en" : "ru")} aria-label={t("Сменить язык")}>{locale === "ru" ? "EN" : "RU"}</button>
        <Hint asChild text={theme === "dark" ? t("Светлая тема") : t("Тёмная тема")}><button className="btn btn--ghost btn--icon" onClick={() => setTheme(theme === "dark" ? "light" : "dark")} aria-label={theme === "dark" ? t("Светлая тема") : t("Тёмная тема")}><Icon name={theme === "dark" ? "sun" : "moon"} size={18} /></button></Hint>
      </div>
    </header>
    <section className="setup-content" aria-busy={busy} data-screen={showingOptions ? "options" : "welcome"}>
      <div className="setup-main setup-view" inert={showingOptions} aria-hidden={showingOptions}>
      <h1 ref={heading} tabIndex={-1} className={phase === "ready" && !busy ? "setup-welcome-title" : undefined}>{phase === "complete" ? t("Sotto установлено") : phase === "failed" ? t("Не удалось установить Sotto") : busy ? t("Устанавливаем Sotto") : "Sotto"}</h1>
      <p className="setup-description">{phase === "complete" ? t("Всё готово. Можно начинать.") : phase === "failed" ? errorText(status?.error ?? null) : busy ? t("Подготавливаем приложение для вашего компьютера.") : t("Мысли становятся текстом.")}</p>
      {(connectionError || actionError) && <p role="alert" className="setup-error">{errorText(actionError)}</p>}
      {busy ? <div className="setup-progress" role="status" aria-live="polite">
        <div className="setup-progress-track" role="progressbar" aria-label={t("Установка Sotto")} />
        <p>{phase === "preparing" ? t("Подготовка файлов") : t("Установка приложения и ярлыков")}</p>
        <span className="setup-caption">{t("Дождитесь завершения. Это окно можно свернуть.")}</span>
      </div> : <div className="setup-actions">
        {phase === "complete"
          ? <button className="btn btn--primary" onClick={() => void launch()} disabled={pending}>{t("Запустить Sotto")}</button>
          : <button className="btn btn--primary" onClick={() => void start()} disabled={!status || !options || connectionError}>{phase === "failed" ? t("Повторить попытку") : t("Установить")}</button>}
        {phase !== "complete" && <button ref={optionsTrigger} className="btn btn--ghost setup-navigation" onClick={openOptions} disabled={!options || connectionError}>{t("Параметры установки")}</button>}
        {native && phase === "complete" && <button className="btn btn--ghost" onClick={() => void closeSetup().catch(() => setActionError("connection_failed"))}>{t("Закрыть")}</button>}
      </div>}
      <div className="setup-status-announcement" role="status" aria-live="polite">{phase === "complete" ? t("Sotto установлено") : ""}</div>
      </div>
      <form className="setup-options setup-view" inert={!showingOptions} aria-hidden={!showingOptions} onSubmit={(event) => { event.preventDefault(); void start(); }}>
        <h2 ref={optionsHeading} tabIndex={-1}>{t("Параметры установки")}</h2>
        <label htmlFor="setup-directory" className="setup-directory-label">{t("Папка приложения")}</label>
        <div className="setup-directory-row">
          <input ref={directoryInput} id="setup-directory" className="field" value={options?.install_dir ?? ""} readOnly={directoryLocked} disabled={choosingDirectory} spellCheck={false} aria-invalid={!!optionsError} aria-describedby={optionsError ? "setup-options-error" : directoryLocked ? "setup-directory-note" : undefined} onChange={(event) => { const install_dir = event.target.value; setOptions(current => current && ({ ...current, install_dir })); setOptionsError(null); }} />
          {!directoryLocked && <button type="button" className="btn" onClick={() => void browse()} disabled={choosingDirectory}>{t("Обзор…")}</button>}
        </div>
        {directoryLocked && <p id="setup-directory-note" className="setup-directory-note">{t("При обновлении используется папка установленной Sotto.")}</p>}
        <div className="setup-shortcuts">
          <label><input className="checkbox" type="checkbox" checked={options?.desktop_shortcut ?? true} onChange={(event) => { const desktop_shortcut = event.target.checked; setOptions(current => current && ({ ...current, desktop_shortcut })); }} />{t("Ярлык на рабочем столе")}</label>
          <label><input className="checkbox" type="checkbox" checked={options?.start_menu_shortcut ?? true} onChange={(event) => { const start_menu_shortcut = event.target.checked; setOptions(current => current && ({ ...current, start_menu_shortcut })); }} />{t("Ярлык в меню «Пуск»")}</label>
        </div>
        {optionsError && <p className="setup-error" role="alert" id="setup-options-error">{errorText(optionsError)}</p>}
        {phase === "failed" && !optionsError && <p className="setup-error" role="alert">{errorText(status?.error ?? null)}</p>}
        <div className="setup-options-actions">
          <button type="button" className="btn btn--ghost setup-navigation" disabled={choosingDirectory} onClick={() => setScreen("welcome")}>{t("Назад")}</button>
          <button type="submit" className="btn btn--primary" disabled={choosingDirectory || !options || connectionError}>{t("Установить")}</button>
        </div>
      </form>
    </section>
    <footer className="setup-footer">
      <div>
        {preview && <p className="setup-preview">{t("Демонстрация · установка не выполняется")}</p>}
        <div className="setup-about" ref={about} onBlur={(event) => { if (!event.currentTarget.contains(event.relatedTarget)) setDetails(false); }}>
          <button ref={aboutButton} id="setup-about-button" className="btn btn--ghost" aria-expanded={details} aria-controls="setup-details" onClick={() => setDetails(!details)}>{t("Об установке")}</button>
          {details && <div id="setup-details" role="region" aria-labelledby="setup-about-button" tabIndex={-1} className="setup-details">{t("Приложение будет установлено для текущего пользователя. Настройки, история и модели сохраняются. Модель распознавания можно выбрать после запуска.")}</div>}
        </div>
      </div>
      {status?.version && <span className="setup-caption setup-version">{status.version}</span>}
    </footer>
  </main>;
}
