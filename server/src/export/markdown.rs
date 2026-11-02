//! Parses the Markdown Peter's documents use into blocks, for Word and Excel
//! export. Mirrors the web renderer: headings, paragraphs, lists, tables,
//! quotes, rules; bold, italic, code; citations [O12] [S2] [D:step] [B:step]; [GAP: ...].

#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    Text(String),
    Bold(Vec<Inline>),
    Italic(Vec<Inline>),
    Code(String),
    Cite(String),
    Gap(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Heading(u8, Vec<Inline>),
    Para(Vec<Inline>),
    List { ordered: bool, items: Vec<Vec<Inline>> },
    Table { head: Vec<Vec<Inline>>, rows: Vec<Vec<Vec<Inline>>> },
    Quote(Vec<Inline>),
    Rule,
}

fn is_cite(s: &str) -> bool {
    let digits = |t: &str| !t.is_empty() && t.chars().all(|c| c.is_ascii_digit());
    s.strip_prefix('O').is_some_and(digits)
        || s.strip_prefix('S').is_some_and(digits)
        || ["D:", "B:"].iter().any(|p| s.strip_prefix(p).is_some_and(|t| !t.is_empty() && t.chars().all(|c| c.is_ascii_lowercase() || c == '_')))
}

pub fn inline(text: &str) -> Vec<Inline> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut i = 0;
    let flush = |buf: &mut String, out: &mut Vec<Inline>| {
        if !buf.is_empty() {
            out.push(Inline::Text(std::mem::take(buf)));
        }
    };
    let find = |from: usize, pat: &[char]| -> Option<usize> {
        (from..chars.len().saturating_sub(pat.len() - 1)).find(|&j| chars[j..j + pat.len()] == *pat)
    };
    while i < chars.len() {
        let c = chars[i];
        if c == '['
            && let Some(end) = find(i + 1, &[']'])
        {
            let inner: String = chars[i + 1..end].iter().collect();
            if let Some(gap) = inner.strip_prefix("GAP:") {
                flush(&mut buf, &mut out);
                out.push(Inline::Gap(gap.trim().to_string()));
                i = end + 1;
                continue;
            }
            if is_cite(&inner) {
                flush(&mut buf, &mut out);
                out.push(Inline::Cite(inner));
                i = end + 1;
                continue;
            }
        }
        if c == '`'
            && let Some(end) = find(i + 1, &['`'])
        {
            flush(&mut buf, &mut out);
            out.push(Inline::Code(chars[i + 1..end].iter().collect()));
            i = end + 1;
            continue;
        }
        if c == '*' && chars.get(i + 1) == Some(&'*')
            && let Some(end) = find(i + 2, &['*', '*'])
            && end > i + 2
        {
            flush(&mut buf, &mut out);
            out.push(Inline::Bold(inline(&chars[i + 2..end].iter().collect::<String>())));
            i = end + 2;
            continue;
        }
        if (c == '*' || c == '_')
            && chars.get(i + 1).is_some_and(|n| *n != c && *n != ' ')
            && !(c == '_' && i > 0 && chars[i - 1].is_alphanumeric())
            && let Some(end) = find(i + 1, &[c])
            && end > i + 1
        {
            flush(&mut buf, &mut out);
            out.push(Inline::Italic(inline(&chars[i + 1..end].iter().collect::<String>())));
            i = end + 1;
            continue;
        }
        buf.push(c);
        i += 1;
    }
    flush(&mut buf, &mut out);
    out
}

fn split_row(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'|') {
            cur.push('|');
            chars.next();
        } else if c == '|' {
            cells.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    cells.push(cur.trim().to_string());
    cells
}

fn is_table_rule(line: &str) -> bool {
    let cells = split_row(line);
    !cells.is_empty()
        && cells.iter().all(|c| {
            let c = c.trim_matches(':');
            c.len() >= 3 && c.chars().all(|ch| ch == '-')
        })
}

fn bullet(line: &str) -> Option<(bool, &str)> {
    let t = line.trim_start();
    for p in ["- ", "* ", "+ "] {
        if let Some(rest) = t.strip_prefix(p) {
            return Some((false, rest));
        }
    }
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        let rest = &t[digits..];
        if let Some(r) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return Some((true, r));
        }
    }
    None
}

fn heading(line: &str) -> Option<(u8, &str)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if (1..=4).contains(&hashes) {
        line[hashes..].strip_prefix(' ').map(|rest| (hashes as u8, rest.trim()))
    } else {
        None
    }
}

fn is_rule(line: &str) -> bool {
    let t: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    t.len() >= 3 && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*') || t.chars().all(|c| c == '_'))
}

pub fn parse(text: &str) -> Vec<Block> {
    let lines: Vec<&str> = text.lines().collect();
    let mut blocks = Vec::new();
    let mut i = 0;
    let table_starts = |i: usize| lines[i].contains('|') && i + 1 < lines.len() && is_table_rule(lines[i + 1]);
    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty() {
            i += 1;
        } else if let Some((level, rest)) = heading(line) {
            blocks.push(Block::Heading(level, inline(rest)));
            i += 1;
        } else if table_starts(i) {
            let head = split_row(line).iter().map(|c| inline(c)).collect();
            i += 2;
            let mut rows = Vec::new();
            while i < lines.len() && lines[i].contains('|') && !lines[i].trim().is_empty() {
                rows.push(split_row(lines[i]).iter().map(|c| inline(c)).collect());
                i += 1;
            }
            blocks.push(Block::Table { head, rows });
        } else if is_rule(line) {
            blocks.push(Block::Rule);
            i += 1;
        } else if let Some((ordered, _)) = bullet(line) {
            let mut items = Vec::new();
            while i < lines.len() {
                let Some((_, rest)) = bullet(lines[i]) else { break };
                let mut item = rest.to_string();
                i += 1;
                while i < lines.len() && lines[i].starts_with("  ") && !lines[i].trim().is_empty() && bullet(lines[i]).is_none() {
                    item.push(' ');
                    item.push_str(lines[i].trim());
                    i += 1;
                }
                items.push(inline(&item));
            }
            blocks.push(Block::List { ordered, items });
        } else if line.starts_with('>') {
            let mut quoted = Vec::new();
            while i < lines.len() && lines[i].starts_with('>') {
                quoted.push(lines[i].trim_start_matches('>').trim());
                i += 1;
            }
            blocks.push(Block::Quote(inline(&quoted.join(" "))));
        } else {
            let mut para = Vec::new();
            while i < lines.len()
                && !lines[i].trim().is_empty()
                && heading(lines[i]).is_none()
                && bullet(lines[i]).is_none()
                && !lines[i].starts_with('>')
                && !table_starts(i)
            {
                para.push(lines[i].trim());
                i += 1;
            }
            blocks.push(Block::Para(inline(&para.join(" "))));
        }
    }
    blocks
}

/// The plain text of inline content, with citations kept as written.
pub fn plain(inlines: &[Inline]) -> String {
    inlines
        .iter()
        .map(|i| match i {
            Inline::Text(t) | Inline::Code(t) => t.clone(),
            Inline::Bold(c) | Inline::Italic(c) => plain(c),
            Inline::Cite(id) => format!("[{id}]"),
            Inline::Gap(g) => format!("[GAP: {g}]"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_shapes_drafts_use() {
        let md = "# Title\n\nIntro **bold** and *it* [O2][S1] [GAP: none].\n\n| ID | Req |\n| --- | --- |\n| URS-001 | Shall \\| work [O3] |\n\n- one\n- two\n\n1. first\n\n> quote\n\n---\n";
        let b = parse(md);
        assert_eq!(b[0], Block::Heading(1, vec![Inline::Text("Title".into())]));
        let Block::Para(p) = &b[1] else { panic!() };
        assert!(p.contains(&Inline::Cite("O2".into())) && p.contains(&Inline::Gap("none".into())));
        assert!(p.contains(&Inline::Bold(vec![Inline::Text("bold".into())])));
        let Block::Table { head, rows } = &b[2] else { panic!("{:?}", b[2]) };
        assert_eq!(head.len(), 2);
        assert_eq!(plain(&rows[0][1]), "Shall | work [O3]");
        assert!(matches!(b[3], Block::List { ordered: false, .. }));
        assert!(matches!(b[4], Block::List { ordered: true, .. }));
        assert!(matches!(b[5], Block::Quote(_)));
        assert_eq!(b[6], Block::Rule);
    }

    #[test]
    fn snake_case_is_not_italic() {
        assert_eq!(inline("audit_trail_on"), vec![Inline::Text("audit_trail_on".into())]);
    }
}
