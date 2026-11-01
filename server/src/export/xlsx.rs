//! Writes every table in a document as a sheet of an Excel (.xlsx) workbook.

use std::io::{Cursor, Write};

use zip::{ZipWriter, write::SimpleFileOptions};

use super::{
    markdown::{Block, parse, plain},
    xml,
};

pub struct Sheet {
    pub name: String,
    pub head: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// The document's tables, each named after the heading above it.
pub fn tables(body: &str) -> Vec<Sheet> {
    let mut sheets: Vec<Sheet> = Vec::new();
    let mut heading = String::new();
    for block in parse(body) {
        match block {
            Block::Heading(_, inl) => heading = plain(&inl),
            Block::Table { head, rows } => {
                let base = if heading.is_empty() { format!("Table {}", sheets.len() + 1) } else { heading.clone() };
                sheets.push(Sheet {
                    name: sheet_name(&base, &sheets),
                    head: head.iter().map(|c| plain(c)).collect(),
                    rows: rows.iter().map(|r| r.iter().map(|c| plain(c)).collect()).collect(),
                });
            }
            _ => {}
        }
    }
    sheets
}

/// Excel sheet names: at most 31 characters, none of []:*?/\ and unique.
fn sheet_name(base: &str, existing: &[Sheet]) -> String {
    let clean: String = base.chars().filter(|c| !"[]:*?/\\".contains(*c)).collect();
    let clean = clean.trim().trim_matches('\'');
    let clean: String = if clean.is_empty() { "Table".into() } else { clean.chars().take(31).collect() };
    let mut name = clean.clone();
    let mut n = 2;
    while existing.iter().any(|s| s.name.eq_ignore_ascii_case(&name)) {
        let suffix = format!(" ({n})");
        name = format!("{}{suffix}", clean.chars().take(31 - suffix.len()).collect::<String>());
        n += 1;
    }
    name
}

fn col(i: usize) -> String {
    let mut n = i + 1;
    let mut s = String::new();
    while n > 0 {
        let r = (n - 1) % 26;
        s.insert(0, (b'A' + r as u8) as char);
        n = (n - 1) / 26;
    }
    s
}

fn sheet_xml(sheet: &Sheet) -> String {
    let cell = |r: usize, c: usize, v: &str, header: bool| {
        let style = if header { r#" s="1""# } else { r#" s="2""# };
        format!(r#"<c r="{}{}" t="inlineStr"{style}><is><t xml:space="preserve">{}</t></is></c>"#, col(c), r, xml(v))
    };
    // Column widths from the longest text, within sensible bounds.
    let widths: Vec<usize> = (0..sheet.head.len())
        .map(|c| {
            let longest = std::iter::once(&sheet.head[c]).chain(sheet.rows.iter().filter_map(|r| r.get(c))).map(|s| s.chars().count()).max().unwrap_or(8);
            longest.clamp(8, 60)
        })
        .collect();
    let mut x = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetViews><sheetView workbookViewId="0"><pane ySplit="1" topLeftCell="A2" activePane="bottomLeft" state="frozen"/></sheetView></sheetViews><cols>"#,
    );
    for (i, w) in widths.iter().enumerate() {
        x.push_str(&format!(r#"<col min="{0}" max="{0}" width="{1}" customWidth="1"/>"#, i + 1, w + 2));
    }
    x.push_str("</cols><sheetData>");
    x.push_str(r#"<row r="1">"#);
    for (c, h) in sheet.head.iter().enumerate() {
        x.push_str(&cell(1, c, h, true));
    }
    x.push_str("</row>");
    for (ri, row) in sheet.rows.iter().enumerate() {
        x.push_str(&format!(r#"<row r="{}">"#, ri + 2));
        for c in 0..sheet.head.len() {
            x.push_str(&cell(ri + 2, c, row.get(c).map(String::as_str).unwrap_or(""), false));
        }
        x.push_str("</row>");
    }
    x.push_str("</sheetData>");
    if !sheet.head.is_empty() {
        x.push_str(&format!(r#"<autoFilter ref="A1:{}{}"/>"#, col(sheet.head.len() - 1), sheet.rows.len() + 1));
    }
    x.push_str("</worksheet>");
    x
}

const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="2"><font><sz val="11"/><name val="Calibri"/></font><font><b/><sz val="11"/><color rgb="FF0F766E"/><name val="Calibri"/></font></fonts><fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FFE2F3F1"/></patternFill></fill></fills><borders count="2"><border/><border><left style="thin"><color rgb="FFC9CFD8"/></left><right style="thin"><color rgb="FFC9CFD8"/></right><top style="thin"><color rgb="FFC9CFD8"/></top><bottom style="thin"><color rgb="FFC9CFD8"/></bottom></border></borders><cellStyleXfs count="1"><xf/></cellStyleXfs><cellXfs count="3"><xf/><xf fontId="1" fillId="2" borderId="1" applyFont="1" applyFill="1" applyBorder="1"><alignment vertical="top" wrapText="1"/></xf><xf borderId="1" applyBorder="1"><alignment vertical="top" wrapText="1"/></xf></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#;

pub fn write(sheets: &[Sheet], title: &str) -> Result<Vec<u8>, String> {
    let mut workbook = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>"#,
    );
    let mut rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    let mut types = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>"#,
    );
    for (i, s) in sheets.iter().enumerate() {
        let n = i + 1;
        workbook.push_str(&format!(r#"<sheet name="{}" sheetId="{n}" r:id="rId{n}"/>"#, xml(&s.name)));
        rels.push_str(&format!(
            r#"<Relationship Id="rId{n}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{n}.xml"/>"#
        ));
        types.push_str(&format!(
            r#"<Override PartName="/xl/worksheets/sheet{n}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#
        ));
    }
    workbook.push_str("</sheets></workbook>");
    rels.push_str(&format!(
        r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#,
        sheets.len() + 1
    ));
    types.push_str("</Types>");
    let root_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>"#;
    let core = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{}</dc:title><dc:description>Exported from Peter</dc:description></cp:coreProperties>"#,
        xml(title)
    );

    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut add = |name: &str, body: &str| -> Result<(), String> {
        zip.start_file(name, opts).map_err(|e| e.to_string())?;
        zip.write_all(body.as_bytes()).map_err(|e| e.to_string())
    };
    add("[Content_Types].xml", &types)?;
    add("_rels/.rels", root_rels)?;
    add("docProps/core.xml", &core)?;
    add("xl/workbook.xml", &workbook)?;
    add("xl/_rels/workbook.xml.rels", &rels)?;
    add("xl/styles.xml", STYLES)?;
    for (i, s) in sheets.iter().enumerate() {
        add(&format!("xl/worksheets/sheet{}.xml", i + 1), &sheet_xml(s))?;
    }
    Ok(zip.finish().map_err(|e| e.to_string())?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheet_names_are_valid_and_unique() {
        let body = "## Requirements: risk/impact\n\n| A |\n| --- |\n| 1 |\n\n## Requirements: risk/impact\n\n| B |\n| --- |\n| 2 |\n\n| C |\n| --- |\n| 3 |\n";
        let s = tables(body);
        let names: Vec<&str> = s.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Requirements riskimpact", "Requirements riskimpact (2)", "Requirements riskimpact (3)"]);
    }

    #[test]
    fn columns_are_lettered() {
        assert_eq!((col(0), col(25), col(26), col(27)), ("A".into(), "Z".into(), "AA".into(), "AB".into()));
    }
}
