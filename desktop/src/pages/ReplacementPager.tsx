import { Icon } from "../components/Icon";
import { t } from "../i18n";

export const REPLACEMENT_PAGE_SIZE = 50;

export function ReplacementPager({ label, page, count, onPage }: { label: string; page: number; count: number; onPage: (page: number) => void }) {
  const pages = Math.max(1, Math.ceil(count / REPLACEMENT_PAGE_SIZE));
  if (pages === 1) return null;
  return <nav className="flex-row" aria-label={label} style={{ gap: 8, justifyContent: "center", padding: "8px 12px", flexWrap: "wrap" }}>
    <button type="button" className="btn btn--ghost btn--sm btn--icon" disabled={page === 0} aria-label={t("Предыдущая страница")} onClick={() => onPage(page - 1)}><Icon name="chev-left" size={12}/></button>
    <span className="dictionary-note">{t("{from}–{to} из {total}", { from: page * REPLACEMENT_PAGE_SIZE + 1, to: Math.min((page + 1) * REPLACEMENT_PAGE_SIZE, count), total: count })}</span>
    <button type="button" className="btn btn--ghost btn--sm btn--icon" disabled={page >= pages - 1} aria-label={t("Следующая страница")} onClick={() => onPage(page + 1)}><Icon name="chev-right" size={12}/></button>
  </nav>;
}
