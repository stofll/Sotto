import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, subscribe } from "../bridge";
import { t } from "../i18n";
import { Icon } from "./Icon";
import { Card } from "./Shell";

export function AccessibilityNotice() {
  const [granted, setGranted] = useState<boolean | null>(null);
  const [checking, setChecking] = useState(false);
  const [failed, setFailed] = useState(false);
  const [dismissed, setDismissed] = useState(false);
  const request = useRef(0);

  const check = useCallback(async () => {
    const id = ++request.current;
    setChecking(true);
    setFailed(false);
    try {
      const result = await invoke<boolean>("check_accessibility");
      if (id === request.current) setGranted(result);
    } catch {
      if (id === request.current) setFailed(true);
    } finally {
      if (id === request.current) setChecking(false);
    }
  }, []);

  useEffect(() => {
    void check();
    const onFocus = () => { void check(); };
    window.addEventListener("focus", onFocus);
    const unsubscribe = subscribe<{ permission?: string }>("app-error", (error) => {
      if (error.permission !== "accessibility") return;
      // A paste denial is newer evidence than an in-flight startup check.
      ++request.current;
      setGranted(false);
      setChecking(false);
      setFailed(false);
    });
    return () => {
      // eslint-disable-next-line react-hooks/exhaustive-deps -- a request generation counter, not a DOM node: cleanup must bump the current value.
      ++request.current;
      unsubscribe();
      window.removeEventListener("focus", onFocus);
    };
  }, [check]);

  if (dismissed || (granted !== false && !failed)) return null;

  const openSettings = async () => {
    try {
      await invoke("open_url", { url: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility" });
    } catch {
      setFailed(true);
    }
  };

  return <Card pad="rows" className="window-banner window-banner--stacked permission-notice accessibility-notice">
    <div className="window-banner__copy" role="alert">
      <strong className="permission-notice__title"><Icon name="info" size={16}/>{t("Для автоматической вставки нужен доступ macOS")}</strong>
      <p>{t("Разрешите Sotto доступ в «Системные настройки → Конфиденциальность и безопасность → Универсальный доступ». Пока доступ не выдан, вставляйте распознанный текст вручную через ⌘V.")}</p>
      <p>{t("Если переключатель Sotto уже включён, удалите приложение из списка и добавьте установленную копию заново. После изменения прав может потребоваться перезапуск Sotto.")}</p>
      {failed && <p>{t("Не удалось проверить доступ или открыть настройки. Попробуйте ещё раз.")}</p>}
    </div>
    <div className="window-banner__actions">
      <button className="btn btn--primary" type="button" onClick={() => void openSettings()}>{t("Открыть настройки macOS")}</button>
      <button className="btn btn--ghost" type="button" disabled={checking} onClick={() => void check()}>{checking ? t("Проверка…") : t("Проверить доступ")}</button>
    </div>
    <button className="btn btn--ghost btn--icon window-banner__close" type="button" aria-label={t("Закрыть")} onClick={() => setDismissed(true)}>
      <Icon name="x" size={14}/>
    </button>
  </Card>;
}
