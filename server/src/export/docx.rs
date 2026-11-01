//! Writes a document version as a Word (.docx) file: cover page with the
//! version's facts and signatures, the body, and appendices listing the
//! evidence it cites (with screenshots) and its version history.

use std::io::{Cursor, Write};

use zip::{ZipWriter, write::SimpleFileOptions};

use super::{
    ExportDoc, image_size,
    markdown::{Block, Inline, parse},
    utc, xml,
};

const ACCENT: &str = "0F766E";
const MUTED: &str = "5D6877";
/// Text width of an A4/Letter page with 2.5 cm margins, in EMU.
const MAX_IMAGE_EMU: i64 = 5_760_000;

#[derive(Default, Clone, Copy)]
struct Fmt {
    bold: bool,
    italic: bool,
}

struct Writer<'a> {
    doc: &'a ExportDoc,
    images: Vec<(String, &'a [u8], &'static str)>, // (rId, bytes, ext)
}

fn run(text: &str, fmt: Fmt, extra: &str) -> String {
    let mut rpr = String::new();
    if fmt.bold {
        rpr.push_str("<w:b/>");
    }
    if fmt.italic {
        rpr.push_str("<w:i/>");
    }
    rpr.push_str(extra);
    format!(r#"<w:r><w:rPr>{rpr}</w:rPr><w:t xml:space="preserve">{}</w:t></w:r>"#, xml(text))
}

fn para(style: &str, runs: &str) -> String {
    let ppr = if style.is_empty() { String::new() } else { format!(r#"<w:pPr><w:pStyle w:val="{style}"/></w:pPr>"#) };
    format!("<w:p>{ppr}{runs}</w:p>")
}

/// An inline picture. `border` draws a thin frame (for screenshots).
fn drawing(rid: &str, cx: i64, cy: i64, id: usize, name: &str, border: bool) -> String {
    let name = xml(name);
    let line = if border { r#"<a:ln w="6350"><a:solidFill><a:srgbClr val="DDE1E7"/></a:solidFill></a:ln>"# } else { "" };
    format!(
        r#"<w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="{cx}" cy="{cy}"/><wp:docPr id="{id}" name="{name}"/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="{id}" name="{name}"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom>{line}</pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing>"#
    )
}

const NAMESPACES: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture""#;

/// The page header: logo on the left, company name on the right.
fn header_xml(b: &super::BrandingOut) -> Option<String> {
    if b.company_name.is_empty() && b.logo.is_none() {
        return None;
    }
    let mut runs = String::new();
    if let Some((_, bytes)) = &b.logo
        && let Some((w, h)) = image_size(bytes)
    {
        // About 1.2 cm tall, at most 6 cm wide.
        let cy = 432_000i64;
        let mut cx = cy * w as i64 / h.max(1) as i64;
        let mut cy2 = cy;
        if cx > 2_160_000 {
            cy2 = cy * 2_160_000 / cx;
            cx = 2_160_000;
        }
        runs.push_str(&format!("<w:r>{}</w:r>", drawing("rIdLogo", cx, cy2, 9001, "Logo", false)));
    }
    if !b.company_name.is_empty() {
        runs.push_str(r#"<w:r><w:tab/></w:r>"#);
        runs.push_str(&run(&b.company_name, Fmt { bold: true, italic: false }, r#"<w:color w:val="17202B"/><w:sz w:val="20"/>"#));
    }
    Some(format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:hdr {NAMESPACES}><w:p><w:pPr><w:pStyle w:val="Footer"/><w:pBdr><w:bottom w:val="single" w:sz="4" w:space="4" w:color="DDE1E7"/></w:pBdr><w:spacing w:after="120"/></w:pPr>{runs}</w:p></w:hdr>"#
    ))
}

fn page_break() -> &'static str {
    r#"<w:p><w:r><w:br w:type="page"/></w:r></w:p>"#
}

impl<'a> Writer<'a> {
    fn runs(&self, inlines: &[Inline], fmt: Fmt) -> String {
        let mut out = String::new();
        for i in inlines {
            match i {
                Inline::Text(t) => out.push_str(&run(t, fmt, "")),
                Inline::Bold(c) => out.push_str(&self.runs(c, Fmt { bold: true, ..fmt })),
                Inline::Italic(c) => out.push_str(&self.runs(c, Fmt { italic: true, ..fmt })),
                Inline::Code(t) => out.push_str(&run(t, fmt, r#"<w:rFonts w:ascii="Consolas" w:hAnsi="Consolas"/><w:sz w:val="19"/>"#)),
                Inline::Cite(id) => {
                    let label = match id.strip_prefix("D:") {
                        Some(step) => format!("[see {}]", self.doc.document_titles.get(step).map(String::as_str).unwrap_or(step)),
                        None => format!("[{id}]"),
                    };
                    out.push_str(&run(&label, Fmt::default(), &format!(r#"<w:color w:val="{ACCENT}"/><w:sz w:val="17"/>"#)));
                }
                Inline::Gap(g) => {
                    out.push_str(&run(&format!("GAP: {g}"), Fmt { bold: true, ..fmt }, r#"<w:highlight w:val="yellow"/>"#));
                }
            }
        }
        out
    }

    fn table(&self, head: &[String], rows: &[Vec<String>]) -> String {
        // Cells already rendered as paragraph XML.
        let cell = |content: &str, header: bool| {
            let shade = if header { r#"<w:shd w:val="clear" w:color="auto" w:fill="E2F3F1"/>"# } else { "" };
            format!(r#"<w:tc><w:tcPr>{shade}</w:tcPr>{content}</w:tc>"#)
        };
        let mut t = String::from(
            r#"<w:tbl><w:tblPr><w:tblStyle w:val="PeterTable"/><w:tblW w:w="5000" w:type="pct"/><w:tblLook w:val="04A0" w:firstRow="1"/></w:tblPr>"#,
        );
        t.push_str(r#"<w:tr><w:trPr><w:tblHeader/><w:cantSplit/></w:trPr>"#);
        for h in head {
            t.push_str(&cell(h, true));
        }
        t.push_str("</w:tr>");
        for r in rows {
            t.push_str(r#"<w:tr><w:trPr><w:cantSplit/></w:trPr>"#);
            for (j, _) in head.iter().enumerate() {
                t.push_str(&cell(r.get(j).map(String::as_str).unwrap_or(r#"<w:p/>"#), false));
            }
            t.push_str("</w:tr>");
        }
        t.push_str("</w:tbl>");
        t.push_str(&para("", "")); // Word needs a paragraph after a table.
        t
    }

    fn simple_table(&self, head: &[&str], rows: &[Vec<String>]) -> String {
        let cell = |s: &str, bold: bool| para("TableText", &run(s, Fmt { bold, italic: false }, ""));
        let head: Vec<String> = head.iter().map(|h| cell(h, true)).collect();
        let rows: Vec<Vec<String>> = rows.iter().map(|r| r.iter().map(|c| cell(c, false)).collect()).collect();
        self.table(&head, &rows)
    }

    fn body(&self) -> String {
        let mut out = String::new();
        for block in parse(&self.doc.body) {
            match block {
                Block::Heading(level, inl) => {
                    out.push_str(&para(&format!("Heading{}", level.min(3)), &self.runs(&inl, Fmt::default())));
                }
                Block::Para(inl) => out.push_str(&para("", &self.runs(&inl, Fmt::default()))),
                Block::Quote(inl) => out.push_str(&para("Quote", &self.runs(&inl, Fmt::default()))),
                Block::Rule => out.push_str(r#"<w:p><w:pPr><w:pBdr><w:bottom w:val="single" w:sz="6" w:space="1" w:color="DDE1E7"/></w:pBdr></w:pPr></w:p>"#),
                Block::List { ordered, items } => {
                    for (n, item) in items.iter().enumerate() {
                        let marker = if ordered { format!("{}.\t", n + 1) } else { "•\t".to_string() };
                        out.push_str(&para("ListPara", &(run(&marker, Fmt::default(), "") + &self.runs(item, Fmt::default()))));
                    }
                }
                Block::Table { head, rows } => {
                    let cell = |c: &[Inline], bold: bool| para("TableText", &self.runs(c, Fmt { bold, italic: false }));
                    let head: Vec<String> = head.iter().map(|c| cell(c, true)).collect();
                    let rows: Vec<Vec<String>> = rows.iter().map(|r| r.iter().map(|c| cell(c, false)).collect()).collect();
                    out.push_str(&self.table(&head, &rows));
                }
            }
        }
        out
    }

    fn image(&mut self, bytes: &'a [u8], content_type: &str, name: &str) -> String {
        let Some((w, h)) = image_size(bytes) else { return String::new() };
        let ext = if content_type == "image/png" { "png" } else { "jpeg" };
        let n = self.images.len() + 1;
        let rid = format!("rIdImg{n}");
        self.images.push((rid.clone(), bytes, ext));
        // Fit the page width, keep proportions, and never upscale.
        let px_emu = 9525i64;
        let (mut cx, mut cy) = (w as i64 * px_emu, h as i64 * px_emu);
        if cx > MAX_IMAGE_EMU {
            cy = cy * MAX_IMAGE_EMU / cx;
            cx = MAX_IMAGE_EMU;
        }
        format!(r#"<w:p><w:pPr><w:keepNext/></w:pPr><w:r>{}</w:r></w:p>"#, drawing(&rid, cx, cy, n, name, true))
    }

    fn document_xml(&mut self) -> String {
        let d = self.doc;
        let mut x = String::new();

        // Cover page.
        x.push_str(&para("Title", &run(&d.title, Fmt::default(), "")));
        let subtitle = [d.project_name.as_str(), d.system_name.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" · ");
        if !subtitle.is_empty() {
            x.push_str(&para("Subtitle", &run(&subtitle, Fmt::default(), "")));
        }
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
        x.push_str(&self.simple_table(&["Item", "Value"], &facts));

        x.push_str(&para("Heading2", &run("Signatures", Fmt::default(), "")));
        if d.signatures.is_empty() {
            x.push_str(&para("", &run("This version has not been signed in Peter. Sign below if signing on paper.", Fmt { italic: true, bold: false }, &format!(r#"<w:color w:val="{MUTED}"/>"#))));
            let blank = vec![String::new(), String::new(), String::new(), String::new()];
            x.push_str(&self.simple_table(&["Name", "Role and meaning", "Signature", "Date"], &[blank.clone(), blank.clone(), blank]));
        } else {
            let rows: Vec<Vec<String>> = d
                .signatures
                .iter()
                .map(|s| vec![format!("{} ({})", s.name, s.email), s.meaning.clone(), format!("Signed electronically in Peter, {}", utc(s.signed_at))])
                .collect();
            x.push_str(&self.simple_table(&["Signed by", "Meaning", "Signature"], &rows));
        }
        if d.gaps > 0 || !d.unknown.is_empty() {
            let mut warn = String::new();
            if d.gaps > 0 {
                warn.push_str(&format!("This version has {} open gap(s) needing evidence. ", d.gaps));
            }
            if !d.unknown.is_empty() {
                warn.push_str(&format!("It cites evidence that does not exist: {}.", d.unknown.join(", ")));
            }
            x.push_str(&para("", &run(warn.trim(), Fmt { bold: true, italic: false }, r#"<w:color w:val="B42318"/>"#)));
        }
        x.push_str(page_break());

        // The document itself.
        x.push_str(&self.body());

        // Appendix A: observed steps cited, with screenshots.
        x.push_str(page_break());
        x.push_str(&para("Heading1", &run("Appendix A. Observed steps cited", Fmt::default(), "")));
        if d.observations.is_empty() {
            x.push_str(&para("", &run("This version cites no observed steps.", Fmt::default(), "")));
        } else {
            let rows: Vec<Vec<String>> = d
                .observations
                .iter()
                .map(|o| vec![format!("O{}", o.seq), o.description.clone(), o.by.clone(), utc(o.captured_at_ms / 1000)])
                .collect();
            x.push_str(&self.simple_table(&["Ref", "What happened", "By", "When"], &rows));
            for o in &d.observations {
                if let Some((content_type, bytes)) = &o.screenshot {
                    let img = self.image(bytes, content_type, &format!("Step {}", o.seq));
                    if !img.is_empty() {
                        x.push_str(&img);
                        x.push_str(&para("Caption", &run(&format!("Figure O{}: {} (SHA-256 {})", o.seq, o.description, o.screenshot_sha256), Fmt::default(), "")));
                    }
                }
            }
        }

        // Appendix B: source files cited.
        x.push_str(&para("Heading1", &run("Appendix B. Source files cited", Fmt::default(), "")));
        if d.sources.is_empty() {
            x.push_str(&para("", &run("This version cites no source files.", Fmt::default(), "")));
        } else {
            let rows: Vec<Vec<String>> = d.sources.iter().map(|s| vec![s.tag.clone(), s.filename.clone(), s.sha256.clone()]).collect();
            x.push_str(&self.simple_table(&["Ref", "File", "SHA-256"], &rows));
        }

        // Appendix C: version history.
        x.push_str(&para("Heading1", &run("Appendix C. Version history", Fmt::default(), "")));
        let rows: Vec<Vec<String>> = d.history.iter().map(|h| vec![h.version.to_string(), h.description.clone(), h.by.clone(), utc(h.at)]).collect();
        x.push_str(&self.simple_table(&["Version", "Made by", "Person", "When"], &rows));

        let header_ref = if header_xml(&d.branding).is_some() { r#"<w:headerReference w:type="default" r:id="rIdHeader"/>"# } else { "" };
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><w:body>{x}<w:sectPr>{header_ref}<w:footerReference w:type="default" r:id="rIdFooter"/><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1418" w:right="1418" w:bottom="1418" w:left="1418" w:header="709" w:footer="709" w:gutter="0"/></w:sectPr></w:body></w:document>"#
        )
    }
}

const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="Calibri" w:cs="Calibri"/><w:sz w:val="21"/><w:szCs w:val="21"/><w:lang w:val="en-GB"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="120" w:line="264" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>
<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>
<w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:before="1200" w:after="120"/></w:pPr><w:rPr><w:b/><w:color w:val="17202B"/><w:sz w:val="48"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Subtitle"><w:name w:val="Subtitle"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:after="480"/></w:pPr><w:rPr><w:color w:val="0F766E"/><w:sz w:val="28"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:pPr><w:keepNext/><w:spacing w:before="360" w:after="120"/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:color w:val="0F766E"/><w:sz w:val="32"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:pPr><w:keepNext/><w:spacing w:before="280" w:after="100"/><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:b/><w:color w:val="17202B"/><w:sz w:val="26"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading3"><w:name w:val="heading 3"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:pPr><w:keepNext/><w:spacing w:before="200" w:after="80"/><w:outlineLvl w:val="2"/></w:pPr><w:rPr><w:b/><w:sz w:val="23"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="ListPara"><w:name w:val="List Paragraph"/><w:basedOn w:val="Normal"/><w:pPr><w:tabs><w:tab w:val="left" w:pos="360"/></w:tabs><w:spacing w:after="60"/><w:ind w:left="360" w:hanging="360"/></w:pPr></w:style>
<w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/><w:basedOn w:val="Normal"/><w:pPr><w:pBdr><w:left w:val="single" w:sz="18" w:space="8" w:color="0F766E"/></w:pBdr><w:ind w:left="240"/></w:pPr><w:rPr><w:i/><w:color w:val="5D6877"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="TableText"><w:name w:val="Table Text"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:before="20" w:after="20" w:line="240" w:lineRule="auto"/></w:pPr><w:rPr><w:sz w:val="18"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Caption"><w:name w:val="caption"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:after="280"/></w:pPr><w:rPr><w:i/><w:color w:val="5D6877"/><w:sz w:val="17"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Footer"><w:name w:val="footer"/><w:basedOn w:val="Normal"/><w:pPr><w:tabs><w:tab w:val="right" w:pos="9070"/></w:tabs><w:spacing w:after="0"/></w:pPr><w:rPr><w:color w:val="5D6877"/><w:sz w:val="16"/></w:rPr></w:style>
<w:style w:type="table" w:styleId="PeterTable"><w:name w:val="Peter Table"/><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4" w:color="C9CFD8"/><w:left w:val="single" w:sz="4" w:color="C9CFD8"/><w:bottom w:val="single" w:sz="4" w:color="C9CFD8"/><w:right w:val="single" w:sz="4" w:color="C9CFD8"/><w:insideH w:val="single" w:sz="4" w:color="C9CFD8"/><w:insideV w:val="single" w:sz="4" w:color="C9CFD8"/></w:tblBorders><w:tblCellMar><w:top w:w="40" w:type="dxa"/><w:left w:w="90" w:type="dxa"/><w:bottom w:w="40" w:type="dxa"/><w:right w:w="90" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style>
</w:styles>"#;

/// "Confidential · ACME-CSV-7F3A9C21-URS · Version 2" (parts present only when set).
fn footer_left(doc: &ExportDoc) -> String {
    let id = if doc.branding.doc_number.is_empty() { doc.title.clone() } else { doc.branding.doc_number.clone() };
    let mut parts = Vec::new();
    if !doc.branding.footer_text.is_empty() {
        parts.push(doc.branding.footer_text.clone());
    }
    parts.push(format!("{id} · Version {}", doc.version));
    parts.join(" · ")
}

pub fn write(doc: &ExportDoc) -> Result<Vec<u8>, String> {
    let mut w = Writer { doc, images: Vec::new() };
    let document = w.document_xml();

    let footer = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:pPr><w:pStyle w:val="Footer"/></w:pPr>{}<w:r><w:tab/></w:r>{}<w:fldSimple w:instr=" PAGE "><w:r><w:t>1</w:t></w:r></w:fldSimple>{}<w:fldSimple w:instr=" NUMPAGES "><w:r><w:t>1</w:t></w:r></w:fldSimple></w:p></w:ftr>"#,
        run(&footer_left(doc), Fmt::default(), ""),
        run("Page ", Fmt::default(), ""),
        run(" of ", Fmt::default(), ""),
    );

    let mut rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdStyles" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rIdFooter" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/>"#,
    );
    let header = header_xml(&doc.branding);
    if header.is_some() {
        rels.push_str(r#"<Relationship Id="rIdHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/>"#);
    }
    for (i, (rid, _, ext)) in w.images.iter().enumerate() {
        rels.push_str(&format!(
            r#"<Relationship Id="{rid}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image{}.{ext}"/>"#,
            i + 1
        ));
    }
    rels.push_str("</Relationships>");

    let header_type = if header.is_some() {
        r#"<Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/>"#
    } else {
        ""
    };
    let content_types = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="jpeg" ContentType="image/jpeg"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/>{header_type}<Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/></Types>"#
    );
    let root_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>"#;
    let core = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>{}</dc:title><dc:creator>{}</dc:creator><cp:revision>{}</cp:revision><dc:description>Exported from Peter</dc:description></cp:coreProperties>"#,
        xml(&doc.title),
        xml(&doc.exported_by),
        doc.version
    );

    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut add = |name: &str, bytes: &[u8], o: SimpleFileOptions| -> Result<(), String> {
        zip.start_file(name, o).map_err(|e| e.to_string())?;
        zip.write_all(bytes).map_err(|e| e.to_string())
    };
    add("[Content_Types].xml", content_types.as_bytes(), opts)?;
    add("_rels/.rels", root_rels.as_bytes(), opts)?;
    add("docProps/core.xml", core.as_bytes(), opts)?;
    add("word/document.xml", document.as_bytes(), opts)?;
    add("word/styles.xml", STYLES.as_bytes(), opts)?;
    add("word/footer1.xml", footer.as_bytes(), opts)?;
    if let Some(h) = &header {
        add("word/header1.xml", h.as_bytes(), opts)?;
        if let Some((kind, bytes)) = &doc.branding.logo {
            let ext = if kind == "image/png" { "png" } else { "jpeg" };
            let hrels = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdLogo" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/logo.{ext}"/></Relationships>"#
            );
            add("word/_rels/header1.xml.rels", hrels.as_bytes(), opts)?;
            add(&format!("word/media/logo.{ext}"), bytes, stored)?;
        }
    }
    add("word/_rels/document.xml.rels", rels.as_bytes(), opts)?;
    for (i, (_, bytes, ext)) in w.images.iter().enumerate() {
        // Images are already compressed.
        add(&format!("word/media/image{}.{ext}", i + 1), bytes, stored)?;
    }
    Ok(zip.finish().map_err(|e| e.to_string())?.into_inner())
}
