// A small Markdown renderer for drafted documents. It builds React elements
// (never raw HTML), so text from an AI cannot inject markup or scripts.
// Supports headings, paragraphs, lists, tables, quotes, bold, italic, code,
// and Peter's evidence citations: [O12], [S2], [D:step], [B:step] and [GAP: ...].

import type { ReactNode } from "react";

export interface CitationHandlers {
  /** Renders an evidence chip such as [O12], [S2] or [D:requirements]. */
  chip: (id: string, key: string) => ReactNode;
}

const CITATION = /\[(O\d+|S\d+|[DB]:[a-z_]+)\]/y;
const GAP = /\[GAP:\s*([^\]]*)\]/y;

function inline(text: string, h: CitationHandlers, keyBase: string): ReactNode[] {
  const out: ReactNode[] = [];
  let buf = "";
  let i = 0;
  let n = 0;
  const flush = () => {
    if (buf) out.push(buf);
    buf = "";
  };
  const key = () => `${keyBase}-${n++}`;

  while (i < text.length) {
    const c = text[i];
    if (c === "[") {
      CITATION.lastIndex = i;
      const cite = CITATION.exec(text);
      if (cite) {
        flush();
        out.push(h.chip(cite[1], key()));
        i = CITATION.lastIndex;
        continue;
      }
      GAP.lastIndex = i;
      const gap = GAP.exec(text);
      if (gap) {
        flush();
        out.push(
          <span key={key()} className="gap" title="Evidence needed">
            Gap: {gap[1]}
          </span>,
        );
        i = GAP.lastIndex;
        continue;
      }
    }
    if (c === "`") {
      const end = text.indexOf("`", i + 1);
      if (end > i) {
        flush();
        out.push(<code key={key()}>{text.slice(i + 1, end)}</code>);
        i = end + 1;
        continue;
      }
    }
    if (c === "*" && text[i + 1] === "*") {
      const end = text.indexOf("**", i + 2);
      if (end > i + 2) {
        flush();
        out.push(<strong key={key()}>{inline(text.slice(i + 2, end), h, key())}</strong>);
        i = end + 2;
        continue;
      }
    }
    if ((c === "*" || c === "_") && text[i + 1] !== c && text[i + 1] !== " ") {
      const end = text.indexOf(c, i + 1);
      // Underscores inside words (snake_case) are not emphasis.
      const wordBefore = c === "_" && /\w/.test(text[i - 1] ?? "");
      if (end > i + 1 && !wordBefore) {
        flush();
        out.push(<em key={key()}>{inline(text.slice(i + 1, end), h, key())}</em>);
        i = end + 1;
        continue;
      }
    }
    buf += c;
    i++;
  }
  flush();
  return out;
}

const splitRow = (line: string) =>
  line
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split(/(?<!\\)\|/)
    .map((cell) => cell.trim().replace(/\\\|/g, "|"));

const isTableRule = (line: string) => /^\s*\|?\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)*\|?\s*$/.test(line);

export function Markdown({ text, handlers }: { text: string; handlers: CitationHandlers }) {
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  const blocks: ReactNode[] = [];
  let i = 0;
  let k = 0;
  const key = () => `b${k++}`;

  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim()) {
      i++;
      continue;
    }

    const heading = /^(#{1,4})\s+(.*)$/.exec(line);
    if (heading) {
      const level = Math.min(heading[1].length + 1, 5); // The page already has an h1.
      const Tag = `h${level}` as "h2";
      const id = key();
      blocks.push(<Tag key={id}>{inline(heading[2], handlers, id)}</Tag>);
      i++;
      continue;
    }

    if (line.includes("|") && i + 1 < lines.length && isTableRule(lines[i + 1])) {
      const head = splitRow(line);
      const rows: string[][] = [];
      i += 2;
      while (i < lines.length && lines[i].includes("|") && lines[i].trim()) {
        rows.push(splitRow(lines[i]));
        i++;
      }
      const id = key();
      blocks.push(
        <div className="table-wrap" key={id}>
          <table className="doc-table">
            <thead>
              <tr>{head.map((c, j) => <th key={j}>{inline(c, handlers, `${id}h${j}`)}</th>)}</tr>
            </thead>
            <tbody>
              {rows.map((r, ri) => (
                <tr key={ri}>{head.map((_, j) => <td key={j}>{inline(r[j] ?? "", handlers, `${id}r${ri}c${j}`)}</td>)}</tr>
              ))}
            </tbody>
          </table>
        </div>,
      );
      continue;
    }

    const bullet = /^\s*([-*+]|\d+[.)])\s+/;
    if (bullet.test(line)) {
      const ordered = /^\s*\d/.test(line);
      const items: string[] = [];
      while (i < lines.length && bullet.test(lines[i])) {
        let item = lines[i].replace(bullet, "");
        i++;
        // Indented continuation lines belong to the item.
        while (i < lines.length && /^\s{2,}\S/.test(lines[i]) && !bullet.test(lines[i])) {
          item += " " + lines[i].trim();
          i++;
        }
        items.push(item);
      }
      const id = key();
      const children = items.map((it, j) => <li key={j}>{inline(it, handlers, `${id}-${j}`)}</li>);
      blocks.push(ordered ? <ol key={id}>{children}</ol> : <ul key={id}>{children}</ul>);
      continue;
    }

    if (line.startsWith(">")) {
      const quoted: string[] = [];
      while (i < lines.length && lines[i].startsWith(">")) {
        quoted.push(lines[i].replace(/^>\s?/, ""));
        i++;
      }
      const id = key();
      blocks.push(<blockquote key={id}>{inline(quoted.join(" "), handlers, id)}</blockquote>);
      continue;
    }

    if (/^\s*(?:-\s*){3,}$|^\s*(?:\*\s*){3,}$|^\s*(?:_\s*){3,}$/.test(line)) {
      blocks.push(<hr key={key()} />);
      i++;
      continue;
    }

    // A paragraph runs until a blank line or another kind of block.
    const para: string[] = [];
    while (
      i < lines.length &&
      lines[i].trim() &&
      !/^#{1,4}\s/.test(lines[i]) &&
      !bullet.test(lines[i]) &&
      !lines[i].startsWith(">") &&
      !(lines[i].includes("|") && i + 1 < lines.length && isTableRule(lines[i + 1]))
    ) {
      para.push(lines[i].trim());
      i++;
    }
    const id = key();
    blocks.push(<p key={id}>{inline(para.join(" "), handlers, id)}</p>);
  }

  return <div className="doc-body">{blocks}</div>;
}
