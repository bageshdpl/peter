//! Writes a document version as a PDF, with the same content as the Word
//! export: cover page with the version's facts and signatures, the body, and
//! appendices with the cited evidence (screenshots included) and the version
//! history. Branding goes in the header and footer of every page.
//!
//! Written from scratch: the Liberation Sans fonts are embedded (SIL Open
//! Font License, see fonts/LICENSE-Liberation.txt) so text looks the same
//! everywhere and can be searched and copied.

use std::{collections::BTreeMap, io::Write};

use flate2::{Compression, write::ZlibEncoder};

use super::{
    ExportDoc,
    markdown::{Block, Inline, parse},
    utc,
};

static REGULAR: &[u8] = include_bytes!("../../fonts/LiberationSans-Regular.ttf");
static BOLD: &[u8] = include_bytes!("../../fonts/LiberationSans-Bold.ttf");
static ITALIC: &[u8] = include_bytes!("../../fonts/LiberationSans-Italic.ttf");

// A4, in points.
const PAGE_W: f32 = 595.28;
const PAGE_H: f32 = 841.89;
const MARGIN_X: f32 = 56.0;
const TOP: f32 = 78.0;
const BOTTOM: f32 = 64.0;
const TEXT_W: f32 = PAGE_W - 2.0 * MARGIN_X;

type Rgb = (f32, f32, f32);
const INK: Rgb = (0.09, 0.125, 0.17);
const MUTED: Rgb = (0.365, 0.408, 0.467);
const ACCENT: Rgb = (0.059, 0.463, 0.431);
const DANGER: Rgb = (0.706, 0.137, 0.094);
const LINE: Rgb = (0.867, 0.882, 0.906);
const HEAD_FILL: Rgb = (0.886, 0.953, 0.945);
const GAP_FILL: Rgb = (1.0, 0.93, 0.55);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FaceId {
    Regular = 0,
    Bold = 1,
    Italic = 2,
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct Style {
    face: FaceId,
    size: f32,
    color: Rgb,
    highlight: Option<Rgb>,
}

impl Style {
    fn new(face: FaceId, size: f32, color: Rgb) -> Self {
        Self { face, size, color, highlight: None }
    }
}

#[derive(Clone, Debug)]
struct Span {
    text: String,
    style: Style,
}

fn span(text: impl Into<String>, style: Style) -> Span {
    Span { text: text.into(), style }
}

struct Font {
    data: &'static [u8],
    face: ttf_parser::Face<'static>,
    upem: f32,
    /// Glyphs drawn, and the character each stands for (for copy and search).
    used: BTreeMap<u16, char>,
}

impl Font {
    fn new(data: &'static [u8]) -> Result<Self, String> {
        let face = ttf_parser::Face::parse(data, 0).map_err(|e| format!("font: {e}"))?;
        let upem = face.units_per_em() as f32;
        Ok(Self { data, face, upem, used: BTreeMap::new() })
    }

    fn glyph(&self, c: char) -> (u16, f32) {
        let c = if c == '\t' { ' ' } else { c };
        let gid = self.face.glyph_index(c).or_else(|| self.face.glyph_index('?')).map(|g| g.0).unwrap_or(0);
        let adv = self.face.glyph_hor_advance(ttf_parser::GlyphId(gid)).unwrap_or(0) as f32;
        (gid, adv / self.upem)
    }

    fn width(&self, text: &str, size: f32) -> f32 {
        text.chars().map(|c| self.glyph(c).1).sum::<f32>() * size
    }
}

#[derive(Clone)]
struct Piece {
    text: String,
    style: Style,
    width: f32,
}

#[derive(Clone)]
struct Line {
    pieces: Vec<Piece>,
    height: f32,
    ascent: f32,
}

enum ImageKind {
    Jpeg { components: u8 },
    Flate { color: Vec<u8>, space: String, alpha: Option<Vec<u8>> },
}

struct Image {
    data: Vec<u8>,
    w: u32,
    h: u32,
    kind: ImageKind,
}

struct Pdf {
    fonts: [Font; 3],
    pages: Vec<String>,
    cur: String,
    y: f32,
    images: Vec<Image>,
}

fn hex_color(c: Rgb, stroke: bool) -> String {
    format!("{:.3} {:.3} {:.3} {}\n", c.0, c.1, c.2, if stroke { "RG" } else { "rg" })
}

impl Pdf {
    fn new() -> Result<Self, String> {
        Ok(Self {
            fonts: [Font::new(REGULAR)?, Font::new(BOLD)?, Font::new(ITALIC)?],
            pages: Vec::new(),
            cur: String::new(),
            y: PAGE_H - TOP,
            images: Vec::new(),
        })
    }

    fn font(&self, f: FaceId) -> &Font {
        &self.fonts[f as usize]
    }

    fn new_page(&mut self) {
        self.pages.push(std::mem::take(&mut self.cur));
        self.y = PAGE_H - TOP;
    }

    fn ensure(&mut self, h: f32) {
        if self.y - h < BOTTOM && self.y < PAGE_H - TOP - 0.5 {
            self.new_page();
        }
    }

    // ---------- Text layout ----------

    /// Breaks spans into lines no wider than `width`.
    fn wrap(&self, spans: &[Span], width: f32) -> Vec<Line> {
        // Words and spaces, each with its style.
        let mut tokens: Vec<(String, Style, bool)> = Vec::new();
        for s in spans {
            let mut word = String::new();
            for c in s.text.chars() {
                if c == '\n' {
                    if !word.is_empty() {
                        tokens.push((std::mem::take(&mut word), s.style, false));
                    }
                    tokens.push(("\n".into(), s.style, false));
                    continue;
                }
                let c = if c == '\r' || c == '\t' { ' ' } else { c };
                if c == ' ' {
                    if !word.is_empty() {
                        tokens.push((std::mem::take(&mut word), s.style, false));
                    }
                    tokens.push((" ".into(), s.style, true));
                } else {
                    word.push(c);
                }
            }
            if !word.is_empty() {
                tokens.push((word, s.style, false));
            }
        }
        let mut lines = Vec::new();
        let mut line: Vec<Piece> = Vec::new();
        let mut w = 0.0;
        let finish = |line: &mut Vec<Piece>, lines: &mut Vec<Line>| {
            while line.last().is_some_and(|p| p.text == " ") {
                line.pop();
            }
            if line.is_empty() {
                return;
            }
            let size = line.iter().map(|p| p.style.size).fold(0.0, f32::max);
            lines.push(Line { pieces: std::mem::take(line), height: size * 1.32, ascent: size * 0.93 });
        };
        for (text, style, space) in tokens {
            if text == "\n" {
                if line.is_empty() {
                    lines.push(Line { pieces: Vec::new(), height: style.size * 1.32, ascent: style.size * 0.93 });
                } else {
                    finish(&mut line, &mut lines);
                }
                w = 0.0;
                continue;
            }
            let font = self.font(style.face);
            if space {
                if !line.is_empty() {
                    let sw = font.width(" ", style.size);
                    line.push(Piece { text, style, width: sw });
                    w += sw;
                }
                continue;
            }
            let tw = font.width(&text, style.size);
            if w + tw > width && !line.is_empty() {
                finish(&mut line, &mut lines);
                w = 0.0;
            }
            if tw <= width {
                line.push(Piece { text, style, width: tw });
                w += tw;
                continue;
            }
            // A word longer than the line: break it between characters.
            let mut part = String::new();
            let mut pw = 0.0;
            for c in text.chars() {
                let cw = font.glyph(c).1 * style.size;
                if w + pw + cw > width && !(part.is_empty() && line.is_empty()) {
                    if !part.is_empty() {
                        line.push(Piece { text: std::mem::take(&mut part), style, width: pw });
                    }
                    finish(&mut line, &mut lines);
                    w = 0.0;
                    pw = 0.0;
                }
                part.push(c);
                pw += cw;
            }
            if !part.is_empty() {
                line.push(Piece { text: part, style, width: pw });
                w += pw;
            }
        }
        finish(&mut line, &mut lines);
        lines
    }

    /// Draws one line with its top at `top`.
    fn draw_line(&mut self, x: f32, top: f32, line: &Line) {
        let base = top - line.ascent;
        let mut x = x;
        // Neighbouring pieces in the same style are drawn as one, spaces
        // included, so copied text keeps its spaces.
        let mut merged: Vec<Piece> = Vec::new();
        for p in &line.pieces {
            match merged.last_mut() {
                Some(last) if last.style == p.style => {
                    last.text.push_str(&p.text);
                    last.width += p.width;
                }
                _ => merged.push(p.clone()),
            }
        }
        for p in &merged {
            if let Some(h) = p.style.highlight {
                self.cur.push_str(&hex_color(h, false));
                self.cur.push_str(&format!("{:.2} {:.2} {:.2} {:.2} re f\n", x - 1.0, base - p.style.size * 0.22, p.width + 2.0, p.style.size * 1.12));
            }
            self.text(x, base, &p.text, p.style);
            x += p.width;
        }
    }

    fn text(&mut self, x: f32, base: f32, text: &str, style: Style) {
        if text.trim().is_empty() {
            return;
        }
        let font = &mut self.fonts[style.face as usize];
        let mut hex = String::with_capacity(text.len() * 4);
        for c in text.chars() {
            let (gid, _) = font.glyph(c);
            font.used.entry(gid).or_insert(if c == '\t' { ' ' } else { c });
            hex.push_str(&format!("{gid:04X}"));
        }
        self.cur.push_str(&hex_color(style.color, false));
        self.cur.push_str(&format!("BT /F{} {:.2} Tf {:.2} {:.2} Td <{hex}> Tj ET\n", style.face as usize, style.size, x, base));
    }

    /// A paragraph at the cursor, moving to new pages as needed.
    fn para(&mut self, spans: &[Span], indent: f32, after: f32) {
        let lines = self.wrap(spans, TEXT_W - indent);
        for line in &lines {
            self.ensure(line.height);
            let top = self.y;
            self.draw_line(MARGIN_X + indent, top, line);
            self.y -= line.height;
        }
        self.y -= after;
    }

    fn rule(&mut self, x1: f32, x2: f32, y: f32, color: Rgb, width: f32) {
        self.cur.push_str(&hex_color(color, true));
        self.cur.push_str(&format!("{width:.2} w {x1:.2} {y:.2} m {x2:.2} {y:.2} l S\n"));
    }

    // ---------- Tables ----------

    fn column_widths(&self, rows: &[&Vec<Vec<Span>>], cols: usize, pad: f32) -> Vec<f32> {
        let mut natural = vec![0.0f32; cols];
        let mut minimum = vec![0.0f32; cols];
        for row in rows {
            for (j, cell) in row.iter().enumerate().take(cols) {
                let mut total = 0.0;
                for s in cell {
                    let f = self.font(s.style.face);
                    total += f.width(&s.text, s.style.size);
                    for word in s.text.split_whitespace() {
                        minimum[j] = minimum[j].max(f.width(word, s.style.size).min(TEXT_W / 3.0));
                    }
                }
                natural[j] = natural[j].max(total);
            }
        }
        let natural: Vec<f32> = natural.iter().map(|w| (w + 2.0 * pad).min(TEXT_W)).collect();
        let minimum: Vec<f32> = minimum.iter().zip(&natural).map(|(m, n)| (m + 2.0 * pad).max(28.0).min(*n).max(2.0 * pad + 8.0)).collect();
        let sum_nat: f32 = natural.iter().sum();
        if sum_nat <= TEXT_W {
            // Everything fits on one line: share out the spare width.
            let extra = (TEXT_W - sum_nat) / cols as f32;
            return natural.iter().map(|w| w + extra).collect();
        }
        let sum_min: f32 = minimum.iter().sum();
        if sum_min >= TEXT_W {
            return minimum.iter().map(|w| w * TEXT_W / sum_min).collect();
        }
        let spare = TEXT_W - sum_min;
        let want: f32 = natural.iter().zip(&minimum).map(|(n, m)| n - m).sum();
        natural.iter().zip(&minimum).map(|(n, m)| m + (n - m) * spare / want.max(0.001)).collect()
    }

    fn table(&mut self, head: &[Vec<Span>], rows: &[Vec<Vec<Span>>]) {
        let cols = head.len().max(rows.iter().map(Vec::len).max().unwrap_or(0));
        if cols == 0 {
            return;
        }
        let pad = 4.0;
        let head_row = head.to_vec();
        let mut all: Vec<&Vec<Vec<Span>>> = vec![&head_row];
        all.extend(rows.iter());
        let widths = self.column_widths(&all, cols, pad);
        let lay = |pdf: &Self, row: &[Vec<Span>]| -> Vec<Vec<Line>> {
            (0..cols).map(|j| row.get(j).map(|c| pdf.wrap(c, widths[j] - 2.0 * pad)).unwrap_or_default()).collect()
        };
        let head_lines = lay(self, head);
        let head_h = row_height(&head_lines, pad);
        self.ensure(head_h + 20.0);
        self.draw_row(&head_lines, &widths, pad, true);
        for row in rows {
            let mut lines = lay(self, row);
            loop {
                let h = row_height(&lines, pad);
                if self.y - h >= BOTTOM {
                    self.draw_row(&lines, &widths, pad, false);
                    break;
                }
                let body = PAGE_H - TOP - BOTTOM - head_h;
                if h <= body || self.y - BOTTOM < 40.0 {
                    // Start the row on a new page, under the header again.
                    self.new_page();
                    self.draw_row(&head_lines, &widths, pad, true);
                    if h <= body {
                        continue;
                    }
                }
                // Taller than a page: draw what fits, continue on the next.
                let room = self.y - BOTTOM - 2.0 * pad;
                let mut first = Vec::new();
                let mut rest = Vec::new();
                for cell in &lines {
                    let mut used = 0.0;
                    let mut k = 0;
                    while k < cell.len() && used + cell[k].height <= room {
                        used += cell[k].height;
                        k += 1;
                    }
                    first.push(cell[..k].to_vec());
                    rest.push(cell[k..].to_vec());
                }
                self.draw_row(&first, &widths, pad, false);
                self.new_page();
                self.draw_row(&head_lines, &widths, pad, true);
                lines = rest;
                if lines.iter().all(Vec::is_empty) {
                    break;
                }
            }
        }
        self.y -= 8.0;
    }

    fn draw_row(&mut self, cells: &[Vec<Line>], widths: &[f32], pad: f32, header: bool) {
        let h = row_height(cells, pad);
        let top = self.y;
        let mut x = MARGIN_X;
        if header {
            self.cur.push_str(&hex_color(HEAD_FILL, false));
            self.cur.push_str(&format!("{:.2} {:.2} {:.2} {:.2} re f\n", MARGIN_X, top - h, TEXT_W, h));
        }
        for (j, cell) in cells.iter().enumerate() {
            let mut ty = top - pad;
            for line in cell {
                self.draw_line(x + pad, ty, line);
                ty -= line.height;
            }
            self.cur.push_str(&hex_color(LINE, true));
            self.cur.push_str(&format!("0.5 w {:.2} {:.2} {:.2} {:.2} re S\n", x, top - h, widths[j], h));
            x += widths[j];
        }
        self.y -= h;
    }

    fn simple_table(&mut self, head: &[&str], rows: &[Vec<String>]) {
        let hs = Style::new(FaceId::Bold, 8.5, INK);
        let bs = Style::new(FaceId::Regular, 8.5, INK);
        let head: Vec<Vec<Span>> = head.iter().map(|h| vec![span(*h, hs)]).collect();
        let rows: Vec<Vec<Vec<Span>>> = rows.iter().map(|r| r.iter().map(|c| vec![span(c.as_str(), bs)]).collect()).collect();
        self.table(&head, &rows);
    }

    // ---------- Images ----------

    fn add_image(&mut self, bytes: &[u8], content_type: &str) -> Option<usize> {
        let img = if content_type == "image/png" || bytes.starts_with(b"\x89PNG") { png(bytes)? } else { jpeg(bytes)? };
        self.images.push(img);
        Some(self.images.len() - 1)
    }

    /// Draws an image at the cursor, scaled to fit, with a thin frame.
    fn image(&mut self, index: usize, max_h: f32, frame: bool) {
        let (w, h) = (self.images[index].w as f32, self.images[index].h as f32);
        let mut dw = (w * 0.75).min(TEXT_W);
        let mut dh = h * dw / w;
        if dh > max_h {
            dw *= max_h / dh;
            dh = max_h;
        }
        self.ensure(dh + 4.0);
        let y = self.y - dh;
        self.cur.push_str(&format!("q {dw:.2} 0 0 {dh:.2} {MARGIN_X:.2} {y:.2} cm /Im{index} Do Q\n"));
        if frame {
            self.cur.push_str(&hex_color(LINE, true));
            self.cur.push_str(&format!("0.5 w {MARGIN_X:.2} {y:.2} {dw:.2} {dh:.2} re S\n"));
        }
        self.y = y - 4.0;
    }

    // ---------- Header and footer ----------

    fn furniture(&mut self, doc: &ExportDoc, logo: Option<usize>) {
        let total = self.pages.len();
        let small = Style::new(FaceId::Regular, 8.0, MUTED);
        for i in 0..total {
            let mut ops = String::new();
            std::mem::swap(&mut ops, &mut self.cur);
            let b = &doc.branding;
            let has_header = logo.is_some() || !b.company_name.is_empty();
            if has_header {
                let line_y = PAGE_H - 52.0;
                if let Some(li) = logo {
                    let (w, h) = (self.images[li].w as f32, self.images[li].h as f32);
                    let mut dh = 22.0;
                    let mut dw = w * dh / h.max(1.0);
                    if dw > 130.0 {
                        dh *= 130.0 / dw;
                        dw = 130.0;
                    }
                    self.cur.push_str(&format!("q {dw:.2} 0 0 {dh:.2} {MARGIN_X:.2} {:.2} cm /Im{li} Do Q\n", line_y + 6.0));
                }
                if !b.company_name.is_empty() {
                    let st = Style::new(FaceId::Bold, 9.0, INK);
                    let name = fit(&self.fonts[1], &b.company_name, 9.0, TEXT_W * 0.6);
                    let w = self.font(FaceId::Bold).width(&name, 9.0);
                    self.text(PAGE_W - MARGIN_X - w, line_y + 9.0, &name, st);
                }
                self.rule(MARGIN_X, PAGE_W - MARGIN_X, line_y, LINE, 0.6);
            }
            let fy = 40.0;
            self.rule(MARGIN_X, PAGE_W - MARGIN_X, fy + 12.0, LINE, 0.6);
            let mut right = format!("Page {} of {total}", i + 1);
            if !b.doc_number.is_empty() {
                right = format!("{} · {right}", b.doc_number);
            }
            let rw = self.font(FaceId::Regular).width(&right, 8.0);
            self.text(PAGE_W - MARGIN_X - rw, fy, &right, small);
            let left = if b.footer_text.is_empty() { format!("{} · version {}", doc.title, doc.version) } else { b.footer_text.clone() };
            let left = fit(&self.fonts[0], &left, 8.0, TEXT_W - rw - 16.0);
            self.text(MARGIN_X, fy, &left, small);
            let furniture = std::mem::replace(&mut self.cur, ops);
            self.pages[i].push_str(&furniture);
        }
    }
}

/// Shortens text with an ellipsis to fit a width.
fn fit(font: &Font, text: &str, size: f32, width: f32) -> String {
    if font.width(text, size) <= width {
        return text.to_string();
    }
    let mut out = String::new();
    for c in text.chars() {
        if font.width(&format!("{out}{c}…"), size) > width {
            break;
        }
        out.push(c);
    }
    format!("{}…", out.trim_end())
}

fn row_height(cells: &[Vec<Line>], pad: f32) -> f32 {
    cells.iter().map(|c| c.iter().map(|l| l.height).sum::<f32>()).fold(0.0, f32::max).max(9.0) + 2.0 * pad
}

// ---------- Images ----------

fn jpeg(b: &[u8]) -> Option<Image> {
    if !b.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut i = 2;
    while i + 9 < b.len() {
        if b[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = b[i + 1];
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let h = u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32;
            let w = u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32;
            let components = b[i + 9];
            if w == 0 || h == 0 || !matches!(components, 1 | 3) {
                return None;
            }
            return Some(Image { data: b.to_vec(), w, h, kind: ImageKind::Jpeg { components } });
        }
        i += 2 + len;
    }
    None
}

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
    let _ = e.write_all(data);
    e.finish().unwrap_or_default()
}

/// 8-bit, non-interlaced PNGs: grey, RGB, palette, each with or without alpha.
fn png(b: &[u8]) -> Option<Image> {
    use std::io::Read;
    if !b.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }
    let mut i = 8;
    let (mut w, mut h, mut depth, mut ctype, mut interlace) = (0u32, 0u32, 0u8, 0u8, 0u8);
    let mut palette: Vec<u8> = Vec::new();
    let mut idat: Vec<u8> = Vec::new();
    while i + 8 <= b.len() {
        let len = u32::from_be_bytes(b[i..i + 4].try_into().ok()?) as usize;
        let kind = &b[i + 4..i + 8];
        let data = b.get(i + 8..i + 8 + len)?;
        match kind {
            b"IHDR" => {
                w = u32::from_be_bytes(data[0..4].try_into().ok()?);
                h = u32::from_be_bytes(data[4..8].try_into().ok()?);
                depth = data[8];
                ctype = data[9];
                interlace = data[12];
            }
            b"PLTE" => palette = data.to_vec(),
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        i += 12 + len;
    }
    if depth != 8 || interlace != 0 || w == 0 || h == 0 || w > 10_000 || h > 10_000 {
        return None;
    }
    let channels = match ctype {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return None,
    };
    let mut raw = Vec::new();
    flate2::read::ZlibDecoder::new(&idat[..]).read_to_end(&mut raw).ok()?;
    let stride = w as usize * channels;
    if raw.len() < (stride + 1) * h as usize {
        return None;
    }
    // Undo the per-row filters.
    let mut px = vec![0u8; stride * h as usize];
    for y in 0..h as usize {
        let filter = raw[y * (stride + 1)];
        let src = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
        for x in 0..stride {
            let a = if x >= channels { px[y * stride + x - channels] as i32 } else { 0 };
            let up = if y > 0 { px[(y - 1) * stride + x] as i32 } else { 0 };
            let c = if y > 0 && x >= channels { px[(y - 1) * stride + x - channels] as i32 } else { 0 };
            let v = src[x] as i32
                + match filter {
                    0 => 0,
                    1 => a,
                    2 => up,
                    3 => (a + up) / 2,
                    4 => {
                        let p = a + up - c;
                        let (pa, pb, pc) = ((p - a).abs(), (p - up).abs(), (p - c).abs());
                        if pa <= pb && pa <= pc { a } else if pb <= pc { up } else { c }
                    }
                    _ => return None,
                };
            px[y * stride + x] = (v & 0xFF) as u8;
        }
    }
    let (color, alpha): (Vec<u8>, Option<Vec<u8>>) = match ctype {
        4 | 6 => {
            let cc = channels - 1;
            let mut color = Vec::with_capacity(w as usize * h as usize * cc);
            let mut alpha = Vec::with_capacity(w as usize * h as usize);
            for p in px.chunks(channels) {
                color.extend_from_slice(&p[..cc]);
                alpha.push(p[cc]);
            }
            let opaque = alpha.iter().all(|&a| a == 255);
            (color, if opaque { None } else { Some(alpha) })
        }
        _ => (px, None),
    };
    let space = match ctype {
        0 | 4 => "/DeviceGray".to_string(),
        3 => {
            if palette.is_empty() {
                return None;
            }
            let hex: String = palette.iter().map(|b| format!("{b:02X}")).collect();
            format!("[/Indexed /DeviceRGB {} <{hex}>]", palette.len() / 3 - 1)
        }
        _ => "/DeviceRGB".to_string(),
    };
    Some(Image { data: Vec::new(), w, h, kind: ImageKind::Flate { color: zlib(&color), space, alpha: alpha.map(|a| zlib(&a)) } })
}

// ---------- The document ----------

fn inline_spans(doc: &ExportDoc, inlines: &[Inline], base: Style, out: &mut Vec<Span>) {
    for i in inlines {
        match i {
            Inline::Text(t) => out.push(span(t.as_str(), base)),
            Inline::Bold(c) => inline_spans(doc, c, Style { face: FaceId::Bold, ..base }, out),
            Inline::Italic(c) => {
                let face = if base.face == FaceId::Bold { FaceId::Bold } else { FaceId::Italic };
                inline_spans(doc, c, Style { face, ..base }, out)
            }
            Inline::Code(t) => out.push(span(t.as_str(), Style { color: (0.25, 0.28, 0.33), ..base })),
            Inline::Cite(id) => {
                let label = match id.strip_prefix("D:") {
                    Some(step) => format!("[see {}]", doc.document_titles.get(step).map(String::as_str).unwrap_or(step)),
                    None => format!("[{id}]"),
                };
                out.push(span(label, Style { face: FaceId::Regular, size: base.size * 0.85, color: ACCENT, highlight: None }));
            }
            Inline::Gap(g) => out.push(span(format!("GAP: {g}"), Style { face: FaceId::Bold, highlight: Some(GAP_FILL), ..base })),
        }
    }
}

fn spans(doc: &ExportDoc, inlines: &[Inline], base: Style) -> Vec<Span> {
    let mut out = Vec::new();
    inline_spans(doc, inlines, base, &mut out);
    out
}

fn body(pdf: &mut Pdf, doc: &ExportDoc) {
    let text = Style::new(FaceId::Regular, 10.0, INK);
    for block in parse(&doc.body) {
        match block {
            Block::Heading(level, inl) => {
                let (size, before, color) = match level {
                    1 => (16.0, 12.0, ACCENT),
                    2 => (13.0, 10.0, INK),
                    _ => (11.0, 8.0, INK),
                };
                pdf.y -= before;
                // Keep a heading with the start of what follows it.
                pdf.ensure(size * 1.4 + 30.0);
                pdf.para(&spans(doc, &inl, Style::new(FaceId::Bold, size, color)), 0.0, 4.0);
            }
            Block::Para(inl) => pdf.para(&spans(doc, &inl, text), 0.0, 6.0),
            Block::Quote(inl) => {
                let start = pdf.y;
                let page = pdf.pages.len();
                pdf.para(&spans(doc, &inl, Style { face: FaceId::Italic, color: MUTED, ..text }), 12.0, 0.0);
                if pdf.pages.len() == page {
                    let end = pdf.y;
                    pdf.cur.push_str(&hex_color(LINE, true));
                    pdf.cur.push_str(&format!("2 w {:.2} {start:.2} m {:.2} {end:.2} l S\n", MARGIN_X + 3.0, MARGIN_X + 3.0));
                }
                pdf.y -= 6.0;
            }
            Block::Rule => {
                pdf.ensure(10.0);
                pdf.y -= 4.0;
                let y = pdf.y;
                pdf.rule(MARGIN_X, PAGE_W - MARGIN_X, y, LINE, 0.8);
                pdf.y -= 8.0;
            }
            Block::List { ordered, items } => {
                for (n, item) in items.iter().enumerate() {
                    let marker = if ordered { format!("{}.", n + 1) } else { "•".to_string() };
                    let lines = pdf.wrap(&spans(doc, item, text), TEXT_W - 16.0);
                    for (k, line) in lines.iter().enumerate() {
                        pdf.ensure(line.height);
                        let top = pdf.y;
                        if k == 0 {
                            pdf.text(MARGIN_X + 2.0, top - line.ascent, &marker, text);
                        }
                        pdf.draw_line(MARGIN_X + 16.0, top, line);
                        pdf.y -= line.height;
                    }
                    pdf.y -= 2.0;
                }
                pdf.y -= 4.0;
            }
            Block::Table { head, rows } => {
                let hs = Style::new(FaceId::Bold, 8.5, INK);
                let bs = Style::new(FaceId::Regular, 8.5, INK);
                let head: Vec<Vec<Span>> = head.iter().map(|c| spans(doc, c, hs)).collect();
                let rows: Vec<Vec<Vec<Span>>> = rows.iter().map(|r| r.iter().map(|c| spans(doc, c, bs)).collect()).collect();
                pdf.y -= 2.0;
                pdf.table(&head, &rows);
            }
        }
    }
}

fn heading(pdf: &mut Pdf, text: &str) {
    pdf.y -= 10.0;
    pdf.ensure(60.0);
    pdf.para(&[span(text, Style::new(FaceId::Bold, 14.0, ACCENT))], 0.0, 6.0);
}

fn note(pdf: &mut Pdf, text: &str, color: Rgb, face: FaceId) {
    pdf.para(&[span(text, Style::new(face, 10.0, color))], 0.0, 6.0);
}

fn layout(pdf: &mut Pdf, d: &ExportDoc) {
    // Cover page.
    pdf.y -= 40.0;
    pdf.para(&[span(d.title.as_str(), Style::new(FaceId::Bold, 22.0, INK))], 0.0, 6.0);
    let subtitle = [d.project_name.as_str(), d.system_name.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" · ");
    if !subtitle.is_empty() {
        pdf.para(&[span(subtitle, Style::new(FaceId::Regular, 12.0, MUTED))], 0.0, 4.0);
    }
    pdf.y -= 16.0;
    let mut facts = Vec::new();
    if !d.branding.doc_number.is_empty() {
        facts.push(vec!["Document number".to_string(), d.branding.doc_number.clone()]);
    }
    facts.extend([
        vec!["Workspace".to_string(), d.workspace_name.clone()],
        vec!["Session".to_string(), d.session_title.clone()],
        vec!["Validation type".to_string(), d.type_title.clone()],
        vec!["Step".to_string(), d.step_label.clone()],
        vec!["Document version".to_string(), d.version.to_string()],
        vec!["Version made by".to_string(), format!("{} ({}) on {}", d.version_by, d.version_origin, utc(d.version_at))],
    ]);
    if let Some(model) = &d.model {
        facts.push(vec!["AI provider and model".to_string(), model.clone()]);
    }
    facts.push(vec!["Exported by".to_string(), format!("{} on {}", d.exported_by, utc(d.exported_at))]);
    pdf.simple_table(&["Item", "Value"], &facts);
    heading(pdf, "Signatures");
    if d.signatures.is_empty() {
        note(pdf, "This version has not been signed in Peter. Sign below if signing on paper.", MUTED, FaceId::Italic);
        let blank = vec!["\n\n".to_string(), String::new(), String::new(), String::new()];
        pdf.simple_table(&["Name", "Role and meaning", "Signature", "Date"], &[blank.clone(), blank.clone(), blank]);
    } else {
        let rows: Vec<Vec<String>> = d
            .signatures
            .iter()
            .map(|s| vec![format!("{} ({})", s.name, s.email), s.meaning.clone(), format!("Signed electronically in Peter, {}", utc(s.signed_at))])
            .collect();
        pdf.simple_table(&["Signed by", "Meaning", "Signature"], &rows);
    }
    if d.gaps > 0 || !d.unknown.is_empty() {
        let mut warn = String::new();
        if d.gaps > 0 {
            warn.push_str(&format!("This version has {} open gap(s) needing evidence. ", d.gaps));
        }
        if !d.unknown.is_empty() {
            warn.push_str(&format!("It cites evidence that does not exist: {}.", d.unknown.join(", ")));
        }
        note(pdf, warn.trim(), DANGER, FaceId::Bold);
    }

    // The document.
    pdf.new_page();
    body(pdf, d);

    // Appendix A: observed steps, with screenshots.
    pdf.new_page();
    heading(pdf, "Appendix A. Observed steps cited");
    if d.observations.is_empty() {
        note(pdf, "This version cites no observed steps.", INK, FaceId::Regular);
    } else {
        let rows: Vec<Vec<String>> =
            d.observations.iter().map(|o| vec![format!("O{}", o.seq), o.description.clone(), o.by.clone(), utc(o.captured_at_ms / 1000)]).collect();
        pdf.simple_table(&["Ref", "What happened", "By", "When"], &rows);
        for o in &d.observations {
            let Some((content_type, bytes)) = &o.screenshot else { continue };
            pdf.y -= 6.0;
            match pdf.add_image(bytes, content_type) {
                Some(index) => {
                    pdf.image(index, (PAGE_H - TOP - BOTTOM) * 0.7, true);
                    let caption = format!("Figure O{}: {} (SHA-256 {})", o.seq, o.description, o.screenshot_sha256);
                    pdf.para(&[span(caption, Style::new(FaceId::Italic, 8.0, MUTED))], 0.0, 6.0);
                }
                None => note(pdf, &format!("Screenshot of O{} is in a format the PDF cannot show; see the Word export.", o.seq), MUTED, FaceId::Italic),
            }
        }
    }

    heading(pdf, "Appendix B. Source files cited");
    if d.sources.is_empty() {
        note(pdf, "This version cites no source files.", INK, FaceId::Regular);
    } else {
        let rows: Vec<Vec<String>> = d.sources.iter().map(|s| vec![s.tag.clone(), s.filename.clone(), s.sha256.clone()]).collect();
        pdf.simple_table(&["Ref", "File", "SHA-256"], &rows);
    }

    heading(pdf, "Appendix C. Version history");
    let rows: Vec<Vec<String>> = d.history.iter().map(|h| vec![h.version.to_string(), h.description.clone(), h.by.clone(), utc(h.at)]).collect();
    pdf.simple_table(&["Version", "Made by", "Person", "When"], &rows);
    pdf.new_page();
}

// ---------- File structure ----------

struct Objects {
    items: Vec<Vec<u8>>,
}

impl Objects {
    fn add(&mut self, body: Vec<u8>) -> usize {
        self.items.push(body);
        self.items.len()
    }
    fn reserve(&mut self) -> usize {
        self.add(Vec::new())
    }
    fn set(&mut self, id: usize, body: Vec<u8>) {
        self.items[id - 1] = body;
    }
    fn stream(&mut self, dict: &str, data: &[u8]) -> usize {
        let mut b = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        b.extend_from_slice(data);
        b.extend_from_slice(b"\nendstream");
        self.add(b)
    }
}

fn pdf_text(s: &str) -> String {
    let mut hex = String::from("<FEFF");
    for u in s.encode_utf16() {
        hex.push_str(&format!("{u:04X}"));
    }
    hex.push('>');
    hex
}

fn font_objects(o: &mut Objects, font: &Font, name: &str) -> usize {
    let f = &font.face;
    let scale = 1000.0 / font.upem;
    let file = o.stream(&format!("/Filter /FlateDecode /Length1 {}", font.data.len()), &zlib(font.data));
    let bbox = f.global_bounding_box();
    let flags = if f.is_italic() { 32 + 64 } else { 32 };
    let descriptor = o.add(
        format!(
            "<< /Type /FontDescriptor /FontName /{name} /Flags {flags} /FontBBox [{} {} {} {}] /ItalicAngle {:.1} /Ascent {} /Descent {} /CapHeight {} /StemV {} /FontFile2 {file} 0 R >>",
            (bbox.x_min as f32 * scale) as i32,
            (bbox.y_min as f32 * scale) as i32,
            (bbox.x_max as f32 * scale) as i32,
            (bbox.y_max as f32 * scale) as i32,
            f.italic_angle(),
            (f.ascender() as f32 * scale) as i32,
            (f.descender() as f32 * scale) as i32,
            (f.capital_height().unwrap_or(f.ascender()) as f32 * scale) as i32,
            if f.is_bold() { 120 } else { 80 },
        )
        .into_bytes(),
    );
    let widths: String = font
        .used
        .keys()
        .map(|&gid| {
            let adv = f.glyph_hor_advance(ttf_parser::GlyphId(gid)).unwrap_or(0) as f32 * scale;
            format!("{gid} [{}]", adv.round() as i32)
        })
        .collect::<Vec<_>>()
        .join(" ");
    let cid = o.add(
        format!(
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{name} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {descriptor} 0 R /W [{widths}] /CIDToGIDMap /Identity >>"
        )
        .into_bytes(),
    );
    // Lets readers copy and search the text.
    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let entries: Vec<(&u16, &char)> = font.used.iter().collect();
    for chunk in entries.chunks(100) {
        cmap.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (gid, c) in chunk {
            let mut buf = [0u16; 2];
            let units: String = c.encode_utf16(&mut buf).iter().map(|u| format!("{u:04X}")).collect();
            cmap.push_str(&format!("<{gid:04X}> <{units}>\n"));
        }
        cmap.push_str("endbfchar\n");
    }
    cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    let to_unicode = o.stream("/Filter /FlateDecode", &zlib(cmap.as_bytes()));
    o.add(
        format!(
            "<< /Type /Font /Subtype /Type0 /BaseFont /{name} /Encoding /Identity-H /DescendantFonts [{cid} 0 R] /ToUnicode {to_unicode} 0 R >>"
        )
        .into_bytes(),
    )
}

fn assemble(pdf: Pdf, d: &ExportDoc) -> Vec<u8> {
    let mut o = Objects { items: Vec::new() };
    let catalog = o.reserve();
    let pages_id = o.reserve();

    let names = ["LiberationSans", "LiberationSans-Bold", "LiberationSans-Italic"];
    let mut font_refs = String::new();
    for (i, font) in pdf.fonts.iter().enumerate() {
        if !font.used.is_empty() {
            let id = font_objects(&mut o, font, names[i]);
            font_refs.push_str(&format!("/F{i} {id} 0 R "));
        }
    }
    let mut image_refs = String::new();
    for (i, img) in pdf.images.iter().enumerate() {
        let id = match &img.kind {
            ImageKind::Jpeg { components } => {
                let space = if *components == 1 { "/DeviceGray" } else { "/DeviceRGB" };
                o.stream(
                    &format!("/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace {space} /BitsPerComponent 8 /Filter /DCTDecode", img.w, img.h),
                    &img.data,
                )
            }
            ImageKind::Flate { color, space, alpha } => {
                let smask = alpha.as_ref().map(|a| {
                    o.stream(
                        &format!("/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode", img.w, img.h),
                        a,
                    )
                });
                let smask = smask.map(|id| format!(" /SMask {id} 0 R")).unwrap_or_default();
                o.stream(
                    &format!("/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace {space} /BitsPerComponent 8 /Filter /FlateDecode{smask}", img.w, img.h),
                    color,
                )
            }
        };
        image_refs.push_str(&format!("/Im{i} {id} 0 R "));
    }
    let resources = format!("<< /Font << {font_refs}>> /XObject << {image_refs}>> >>");

    let mut kids = Vec::new();
    for content in &pdf.pages {
        let stream = o.stream("/Filter /FlateDecode", &zlib(content.as_bytes()));
        let page = o.add(
            format!("<< /Type /Page /Parent {pages_id} 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] /Resources {resources} /Contents {stream} 0 R >>").into_bytes(),
        );
        kids.push(format!("{page} 0 R"));
    }
    o.set(pages_id, format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), kids.len()).into_bytes());
    o.set(catalog, format!("<< /Type /Catalog /Pages {pages_id} 0 R >>").into_bytes());
    let date = utc(d.exported_at);
    let stamp: String = date.chars().filter(|c| c.is_ascii_digit()).collect();
    let info = o.add(
        format!(
            "<< /Title {} /Author {} /Subject {} /Creator (Peter) /Producer (Peter) /CreationDate (D:{stamp}00Z) >>",
            pdf_text(&format!("{} v{}", d.title, d.version)),
            pdf_text(&d.exported_by),
            pdf_text(&d.session_title),
        )
        .into_bytes(),
    );

    let mut out: Vec<u8> = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in o.items.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", o.items.len() + 1).as_bytes());
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root {catalog} 0 R /Info {info} 0 R >>\nstartxref\n{xref}\n%%EOF\n", o.items.len() + 1).as_bytes());
    out
}

pub fn write(doc: &ExportDoc) -> Result<Vec<u8>, String> {
    let mut pdf = Pdf::new()?;
    layout(&mut pdf, doc);
    // `layout` ends with a fresh page; drop it if nothing was drawn on it.
    if pdf.cur.is_empty() && pdf.pages.last().is_some_and(String::is_empty) {
        pdf.pages.pop();
    }
    let logo = doc.branding.logo.as_ref().and_then(|(t, b)| pdf.add_image(b, t));
    pdf.furniture(doc, logo);
    Ok(assemble(pdf, doc))
}
