import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { checkUpdate } from "../bridge/updates";
import { subscribeWindowActivity } from "../bridge/windowActivity";
import type { UpdateInfo } from "../bridge/types";
import { t } from "../i18n";
import { Card } from "./Shell";
import { Icon } from "./Icon";
import { UPDATE_CHECK_INTERVAL, UPDATE_NOTICE_DURATION, updateReminderDue } from "./updateReminder";

const DIALOG_SELECTOR = '[role="dialog"], [role="alertdialog"]';

export function UpdateNotice({ receiveBeta, ready, lastShownAt = 0, onShown, onUpdate }: {
  receiveBeta: boolean;
  ready: boolean;
  lastShownAt?: number;
  onShown: (timestamp: number) => void;
  /** Start installing; the Help page shows the download and any error. */
  onUpdate: () => void;
}) {
  const [checked, setChecked] = useState<{ receiveBeta: boolean; info: UpdateInfo } | null>(null);
  const [notice, setNotice] = useState<UpdateInfo | null>(null);
  const [visible, setVisible] = useState(false);
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const [dialogOpen, setDialogOpen] = useState(false);
  const shownAt = useRef(0);
  const windowActive = useRef(false);
  const checkOnActivity = useRef<(() => void) | null>(null);
  const info = checked?.receiveBeta === receiveBeta ? checked.info : null;

  useEffect(() => subscribeWindowActivity((active) => {
    windowActive.current = active;
    setVisible(active);
    checkOnActivity.current?.();
  }), []);

  useEffect(() => {
    let disposed = false;
    let lastCheck = -Infinity;
    let timer: number | undefined;
    setChecked(null);
    setNotice(null);
    async function check() {
      window.clearTimeout(timer);
      const now = Date.now();
      if (!windowActive.current) return;
      const remaining = UPDATE_CHECK_INTERVAL - (now - lastCheck);
      if (remaining > 0) {
        timer = window.setTimeout(() => void check(), remaining);
        return;
      }
      lastCheck = now;
      timer = window.setTimeout(() => void check(), UPDATE_CHECK_INTERVAL);
      try {
        const result = await checkUpdate(receiveBeta);
        if (!disposed) setChecked({ receiveBeta, info: result });
      } catch { /* Automatic checks remain quiet offline; the Help page offers retry. */ }
    }
    checkOnActivity.current = () => { void check(); };
    void check();
    return () => {
      disposed = true;
      window.clearTimeout(timer);
      checkOnActivity.current = null;
    };
  }, [receiveBeta]);

  useLayoutEffect(() => {
    if (!info?.available) return;
    const updateDialogState = () => setDialogOpen(!!document.querySelector(DIALOG_SELECTOR));
    updateDialogState();
    const observer = new MutationObserver(updateDialogState);
    observer.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ["role"] });
    return () => observer.disconnect();
  }, [info?.available]);

  useEffect(() => {
    if (!ready || !visible || dialogOpen || !info?.available || !info.version || notice) return;
    // The DOM can already contain a new dialog before its observer updates state.
    if (document.querySelector(DIALOG_SELECTOR)) return;
    const now = Date.now();
    if (!updateReminderDue(shownAt.current, now) || !updateReminderDue(lastShownAt, now)) return;
    shownAt.current = now;
    onShown(now);
    setNotice(info);
  }, [info, notice, ready, visible, dialogOpen, lastShownAt, onShown]);

  useEffect(() => {
    if (!notice || !ready || !visible || dialogOpen) {
      setHovered(false);
      setFocused(false);
      return;
    }
    if (hovered || focused) return;
    const timer = window.setTimeout(() => setNotice(null), UPDATE_NOTICE_DURATION);
    return () => window.clearTimeout(timer);
  }, [notice, ready, visible, dialogOpen, hovered, focused]);

  if (!info?.available || !notice || !ready || !visible || dialogOpen) return null;
  return <div className="update-notice" data-testid="update-notice"
    onMouseEnter={() => setHovered(true)} onMouseLeave={() => setHovered(false)}
    onFocus={() => setFocused(true)} onBlur={(event) => {
      if (!event.currentTarget.contains(event.relatedTarget)) setFocused(false);
    }}>
    <Card pad="rows" className="update-notice__card">
      <span className="card-icon"><Icon name="download" size={16}/></span>
      <div className="update-notice__copy" role="status">
        <strong>{t("Доступна новая версия Sotto {p0}", { p0: notice.version ?? "" })}</strong>
        <span>{t("Обновитесь, когда будет удобно.")}</span>
      </div>
      <button className="btn btn--primary" type="button" onClick={() => { setNotice(null); onUpdate(); }}><Icon name="download" size={12}/>{t("Обновить сейчас")}</button>
      <button className="btn btn--ghost" type="button" aria-label={t("Закрыть уведомление")} onClick={() => setNotice(null)}><Icon name="x" size={12}/></button>
    </Card>
  </div>;
}
