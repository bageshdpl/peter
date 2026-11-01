//! Plain text from uploaded source files, so drafts can quote and cite them.

use std::io::{Cursor, Read};

/// Text of a file, or a short reason it could not be read. Runs off the async
/// threads because PDF parsing can be slow.
pub async fn text_of(path: std::path::PathBuf, content_type: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let bytes = std::fs::read(&path).map_err(|_| "the stored file is missing".to_string())?;
        // A malformed file must not take the server down.
        std::panic::catch_unwind(|| extract(&bytes, &content_type)).unwrap_or_else(|_| Err("the file could not be parsed".into()))
    })
    .await
    .unwrap_or_else(|_| Err("the file could not be parsed".into()))
}

fn extract(bytes: &[u8], content_type: &str) -> Result<String, String> {
    match content_type {
        "text/plain" | "text/markdown" | "text/csv" => Ok(String::from_utf8_lossy(bytes).into_owned()),
        "application/pdf" => pdf_extract::extract_text_from_mem(bytes).map_err(|_| "the PDF has no readable text".into()),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
            office_xml_text(bytes, |name| name == "word/document.xml")
        }
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => {
            office_xml_text(bytes, |name| name == "xl/sharedStrings.xml" || name.starts_with("xl/worksheets/sheet"))
        }
        _ => Err("images are not read yet".into()),
    }
}

/// Word and Excel files are ZIP archives of XML. Takes the text between tags,
/// treating paragraph, row and cell ends as line or tab breaks.
fn office_xml_text(bytes: &[u8], wanted: impl Fn(&str) -> bool) -> Result<String, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "the file is not a valid Office document".to_string())?;
    let mut out = String::new();
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|_| "the file is damaged".to_string())?;
        let name = file.name().map(|n| n.into_owned()).unwrap_or_default();
        if !wanted(&name) {
            continue;
        }
        // Guard against archives that expand to huge sizes.
        let mut xml = String::new();
        file.by_ref().take(20 * 1024 * 1024).read_to_string(&mut xml).map_err(|_| "the file is damaged".to_string())?;
        out.push_str(&xml_to_text(&xml));
        out.push('\n');
    }
    Ok(out)
}

fn xml_to_text(xml: &str) -> String {
    let mut out = String::new();
    let mut rest = xml;
    while let Some(start) = rest.find('<') {
        out.push_str(&unescape(&rest[..start]));
        let Some(end) = rest[start..].find('>') else { break };
        let tag = &rest[start + 1..start + end];
        match tag.split_whitespace().next().unwrap_or("") {
            "/w:p" | "/row" | "w:br/" | "w:br" => out.push('\n'),
            "/w:tc" | "/c" | "w:tab/" => out.push('\t'),
            "/si" => out.push('\n'),
            _ => {}
        }
        rest = &rest[start + end + 1..];
    }
    out
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// Collapses runs of blank space so excerpts carry more content.
pub fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank_lines = 0;
    for line in text.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            blank_lines += 1;
            if blank_lines == 1 {
                out.push('\n');
            }
        } else {
            blank_lines = 0;
            out.push_str(&line);
            out.push('\n');
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_xml_becomes_lines() {
        let xml = r#"<w:document><w:body><w:p><w:r><w:t>Sample &amp; batch</w:t></w:r></w:p><w:p><w:r><w:t>Line two</w:t></w:r></w:p></w:body></w:document>"#;
        assert_eq!(tidy(&xml_to_text(xml)), "Sample & batch\nLine two");
    }

    #[test]
    fn pdf_text_is_read() {
        let pdf = include_bytes!("../tests/fixtures/manual.pdf");
        let text = extract(pdf, "application/pdf").unwrap();
        assert!(tidy(&text).contains("Audit trail must record every change."), "{text:?}");
    }

    #[test]
    fn garbage_is_an_error_not_a_crash() {
        assert!(extract(b"not a pdf", "application/pdf").is_err());
        assert!(extract(b"not a zip", "application/vnd.openxmlformats-officedocument.wordprocessingml.document").is_err());
    }

    #[test]
    fn tidy_collapses_space() {
        assert_eq!(tidy("a   b\n\n\n\nc  "), "a b\n\nc");
    }
}
