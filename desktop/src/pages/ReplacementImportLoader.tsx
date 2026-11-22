import { useEffect, useState, type ComponentProps } from "react";
import { Modal } from "../components/Modal";
import { t } from "../i18n";

type ImportDialog = typeof import("./ReplacementImportDialog")["ReplacementImportDialog"];

/** Keep a failed optional chunk inside the import flow, without unmounting
 * the editor and losing its unsaved rules. */
export function ReplacementImportLoader(props: ComponentProps<ImportDialog>) {
  const [Dialog, setDialog] = useState<ImportDialog | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void import("./ReplacementImportDialog")
      .then(({ ReplacementImportDialog }) => { if (!cancelled) setDialog(() => ReplacementImportDialog); })
      .catch(() => { if (!cancelled) setFailed(true); });
    return () => { cancelled = true; };
  }, []);

  if (Dialog) return <Dialog {...props}/>;
  return <Modal title={t("Импорт правил замен")} onClose={props.onClose} className="dictionary-modal" focusFirstInput={false}>
    <div className="modal__body">
      <p className="dictionary-note">{props.file.name}</p>
      {failed
        ? <p role="alert" className="inline-error">{t("Не удалось открыть импорт правил. Черновик остался в редакторе; закройте окно импорта, сохраните или экспортируйте правила и перезапустите Sotto.")}</p>
        : <p role="status" className="dictionary-note">{t("Загрузка…")}</p>}
    </div>
    <div className="modal__foot">
      <button type="button" className="btn btn--ghost btn--sm" onClick={props.onClose}>{t("Отмена")}</button>
    </div>
  </Modal>;
}
