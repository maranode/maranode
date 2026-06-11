//! extract text from plain files and PDF with page numbers

use anyhow::{bail, Result};

#[derive(Debug, Clone)]
pub struct Page {
    pub number: u32,
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct DocumentMeta {
    pub title: Option<String>,
    pub author: Option<String>,
    pub page_count: u32,
}

#[derive(Debug, Clone)]
pub struct DocumentContent {
    pub pages: Vec<Page>,
    pub meta: DocumentMeta,
    pub full_text: String,
}

impl DocumentContent {
    pub fn from_plain(text: String) -> Self {
        let full_text = text.clone();
        Self {
            pages: vec![Page { number: 1, text }],
            meta: DocumentMeta {
                page_count: 1,
                ..Default::default()
            },
            full_text,
        }
    }
}

pub fn extract(bytes: &[u8], filename: &str) -> Result<DocumentContent> {
    let ext = std::path::Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "txt" | "md" | "markdown" | "csv" | "log" | "rst" | "text" => {
            let text = std::str::from_utf8(bytes).map_err(|_| {
                anyhow::anyhow!(
                    "file '{}' is not valid UTF-8: only UTF-8 text files are supported",
                    filename
                )
            })?;
            Ok(DocumentContent::from_plain(text.to_string()))
        }
        "pdf" => extract_pdf(bytes, filename),
        "" => bail!(
            "file '{}' has no extension: supported: txt, md, csv, pdf",
            filename
        ),
        other => bail!(
            "unsupported file type '.{}': supported: txt, md, csv, pdf",
            other
        ),
    }
}

pub fn extract_text(bytes: &[u8], filename: &str) -> Result<String> {
    Ok(extract(bytes, filename)?.full_text)
}

fn extract_pdf(bytes: &[u8], filename: &str) -> Result<DocumentContent> {
    let page_texts = pdf_extract::extract_text_from_mem_by_pages(bytes)
        .map_err(|e| anyhow::anyhow!("could not extract text from '{}': {}", filename, e))?;

    let page_count = page_texts.len() as u32;
    let is_empty = page_count == 0 || page_texts.iter().all(|p| p.trim().is_empty());

    if is_empty {
        return try_ocr_pdf(bytes, filename);
    }

    let (title, author) = extract_pdf_meta(bytes);

    let pages: Vec<Page> = page_texts
        .into_iter()
        .enumerate()
        .filter(|(_, t)| !t.trim().is_empty())
        .map(|(i, text)| Page {
            number: (i + 1) as u32,
            text: enrich_with_tables(text),
        })
        .collect();

    let full_text = pages
        .iter()
        .map(|p| p.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");

    Ok(DocumentContent {
        full_text,
        meta: DocumentMeta { title, author, page_count },
        pages,
    })
}

/// when the PDF has no text layer, try running it through `ocrmypdf --force-ocr`
/// which calls Tesseract internally. requires ocrmypdf to be installed.
fn try_ocr_pdf(bytes: &[u8], filename: &str) -> Result<DocumentContent> {
    use std::process::Command;

    let has_ocrmypdf = Command::new("ocrmypdf")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !has_ocrmypdf {
        bail!(
            "PDF '{}' has no text layer (scanned image). \
             Install ocrmypdf to enable automatic OCR: https://ocrmypdf.readthedocs.io",
            filename
        );
    }

    let dir = tempfile::tempdir()?;
    let in_path = dir.path().join("input.pdf");
    let out_path = dir.path().join("output.pdf");
    std::fs::write(&in_path, bytes)?;

    let status = Command::new("ocrmypdf")
        .args([
            "--force-ocr",
            "--quiet",
            "--output-type", "pdf",
            in_path.to_str().unwrap(),
            out_path.to_str().unwrap(),
        ])
        .status()?;

    if !status.success() {
        bail!(
            "ocrmypdf failed on '{}' (exit {}). \
             The PDF may be encrypted or corrupt.",
            filename,
            status
        );
    }

    let ocr_bytes = std::fs::read(&out_path)?;
    let page_texts = pdf_extract::extract_text_from_mem_by_pages(&ocr_bytes)
        .map_err(|e| anyhow::anyhow!("text extraction after OCR failed for '{}': {}", filename, e))?;

    if page_texts.iter().all(|p| p.trim().is_empty()) {
        bail!("OCR produced no text for '{}': document may be blank or unreadable", filename);
    }

    let page_count = page_texts.len() as u32;
    let (title, author) = extract_pdf_meta(bytes);

    let pages: Vec<Page> = page_texts
        .into_iter()
        .enumerate()
        .filter(|(_, t)| !t.trim().is_empty())
        .map(|(i, text)| Page {
            number: (i + 1) as u32,
            text,
        })
        .collect();

    let full_text = pages.iter().map(|p| p.text.as_str()).collect::<Vec<_>>().join("\n\n");

    Ok(DocumentContent {
        full_text,
        meta: DocumentMeta { title, author, page_count },
        pages,
    })
}

/// heuristically detect and convert whitespace-aligned table blocks to Markdown tables.
/// works on text that was extracted from PDF with fixed-width column alignment.
fn enrich_with_tables(text: String) -> String {
    let mut out = String::with_capacity(text.len() + 256);
    let mut table_buf: Vec<Vec<String>> = Vec::new();

    let flush_table = |buf: &mut Vec<Vec<String>>, out: &mut String| {
        if buf.len() < 2 {
            for row in buf.iter() {
                out.push_str(&row.join("  "));
                out.push('\n');
            }
            buf.clear();
            return;
        }
        let cols = buf.iter().map(|r| r.len()).max().unwrap_or(0);
        if cols < 2 {
            for row in buf.iter() {
                out.push_str(&row.join("  "));
                out.push('\n');
            }
            buf.clear();
            return;
        }
        // header row
        out.push_str("| ");
        out.push_str(&buf[0].join(" | "));
        out.push_str(" |\n");
        // separator
        out.push('|');
        for _ in 0..cols {
            out.push_str(" --- |");
        }
        out.push('\n');
        // data rows
        for row in &buf[1..] {
            out.push_str("| ");
            let mut cells: Vec<&str> = row.iter().map(|s| s.as_str()).collect();
            while cells.len() < cols {
                cells.push("");
            }
            out.push_str(&cells.join(" | "));
            out.push_str(" |\n");
        }
        out.push('\n');
        buf.clear();
    };

    for line in text.lines() {
        // a "table-like" line has 2+ runs of whitespace (>=2 spaces) separating tokens
        let cells: Vec<String> = line
            .split("  ")
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .collect();

        if cells.len() >= 2 && line.contains("  ") {
            table_buf.push(cells);
        } else {
            flush_table(&mut table_buf, &mut out);
            out.push_str(line);
            out.push('\n');
        }
    }
    flush_table(&mut table_buf, &mut out);
    out
}

fn extract_pdf_meta(bytes: &[u8]) -> (Option<String>, Option<String>) {
    use lopdf::Document;
    let Ok(doc) = Document::load_mem(bytes) else {
        return (None, None);
    };

    let info_ref = match doc.trailer.get(b"Info") {
        Ok(lopdf::Object::Reference(r)) => *r,
        _ => return (None, None),
    };

    let Ok(lopdf::Object::Dictionary(info)) = doc.get_object(info_ref) else {
        return (None, None);
    };

    let get_str = |key: &[u8]| -> Option<String> {
        let obj = info.get(key).ok()?;
        let s = lopdf::decode_text_string(obj).ok()?;
        let trimmed = s.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    };

    (get_str(b"Title"), get_str(b"Author"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_single_page() {
        let doc = extract(b"Hello world\nSecond line", "doc.txt").unwrap();
        assert_eq!(doc.pages.len(), 1);
        assert_eq!(doc.pages[0].number, 1);
        assert!(doc.full_text.contains("Hello world"));
        assert_eq!(doc.meta.page_count, 1);
    }

    #[test]
    fn unsupported_extension_rejected() {
        assert!(extract(b"data", "file.docx")
            .unwrap_err()
            .to_string()
            .contains("unsupported"));
    }

    #[test]
    fn no_extension_rejected() {
        assert!(extract(b"data", "Makefile")
            .unwrap_err()
            .to_string()
            .contains("no extension"));
    }

    #[test]
    fn legacy_extract_text_works() {
        assert_eq!(extract_text(b"hello", "doc.txt").unwrap(), "hello");
    }
}
