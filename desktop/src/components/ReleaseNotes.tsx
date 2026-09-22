import { Fragment, useMemo, type ReactNode } from "react";
import { lexer, type Token, type Tokens } from "marked";
import { t } from "../i18n";

function webUrl(value: string): string | null {
  try {
    const url = new URL(value);
    return url.protocol === "https:" || url.protocol === "http:" ? url.href : null;
  } catch { return null; }
}

/** Render tokens as React elements; remote HTML and images stay inert text. */
export default function ReleaseNotes({ text, onOpenUrl }: { text: string; onOpenUrl: (url: string) => void }) {
  const tokens = useMemo(() => lexer(text), [text]);
  function render(items: Token[]): ReactNode {
    return items.map((token, index) => {
      const children = () => render("tokens" in token ? token.tokens ?? [] : []);
      let node: ReactNode;
      switch (token.type) {
        case "space": case "def": return null;
        case "heading": node = <h3>{children()}</h3>; break;
        case "paragraph": node = <p>{children()}</p>; break;
        case "text": node = token.tokens ? children() : token.text; break;
        case "escape": node = token.text; break;
        case "image": node = token.text; break;
        case "strong": node = <strong>{children()}</strong>; break;
        case "em": node = <em>{children()}</em>; break;
        case "del": node = <del>{children()}</del>; break;
        case "codespan": node = <code>{token.text}</code>; break;
        case "code": node = <pre><code>{token.text}</code></pre>; break;
        case "br": node = <br/>; break;
        case "hr": node = <hr/>; break;
        case "blockquote": node = <blockquote>{children()}</blockquote>; break;
        case "table": {
          const table = token as Tokens.Table;
          node = <div className="release-notes__table" tabIndex={0} role="region" aria-label={t("Таблица изменений")}>
            <table>
              <thead><tr>{table.header.map((cell, i) =>
                <th key={i} scope="col" data-align={table.align[i]}>{render(cell.tokens)}</th>
              )}</tr></thead>
              <tbody>{table.rows.map((row, i) => <tr key={i}>{row.map((cell, j) =>
                <td key={j} data-align={table.align[j]}>{render(cell.tokens)}</td>
              )}</tr>)}</tbody>
            </table>
          </div>;
          break;
        }
        case "list": {
          const items = (token.items as Token[]).map((item, i) => <li key={i}>{render("tokens" in item ? item.tokens ?? [] : [])}</li>);
          node = token.ordered ? <ol start={token.start}>{items}</ol> : <ul>{items}</ul>;
          break;
        }
        case "link": {
          const url = webUrl(token.href);
          node = url ? <a href={url} onClick={(event) => { event.preventDefault(); onOpenUrl(url); }}
            onAuxClick={(event) => event.preventDefault()}>{children()}</a> : children();
          break;
        }
        default: node = token.raw;
      }
      return <Fragment key={index}>{node}</Fragment>;
    });
  }
  return <div className="release-notes">{render(tokens)}</div>;
}
