import { t } from "../i18n";

export type ProviderDiagnostics = { http_status?: number; response_snippet?: string };

/** The provider's raw response beneath the error. Collapsed: needed when the
 *  error message says "wrong response shape", and unnecessary in every other
 *  case. */
export function ProviderSnippet({ result }: { result: ProviderDiagnostics }) {
  if (!result.response_snippet) return null;
  return (
    <details style={{ marginTop: 2 }}>
      <summary style={{ cursor: "pointer", font: "500 11px/1.4 var(--font-sans)", color: "var(--ink-mute)" }}>
        {result.http_status ? t("Ответ провайдера (HTTP {p0})", { p0: result.http_status }) : t("Ответ провайдера")}
      </summary>
      <pre className="scroll-visible" style={{ margin: "6px 0 0", padding: 10, maxHeight: 180, overflow: "auto", borderRadius: "var(--radius-sm)", background: "var(--bg-2)", border: "1px solid var(--line)", font: "400 11px/1.5 var(--font-mono)", color: "var(--ink-mute)", whiteSpace: "pre-wrap", wordBreak: "break-word" }}>
        {result.response_snippet}
      </pre>
    </details>
  );
}
