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
    // One string per PDF page
    let page_texts = pdf_extract::extract_text_from_mem_by_pages(bytes)
        .map_err(|e| anyhow::anyhow!("could not extract text from '{}': {}", filename, e))?;

    let page_count = page_texts.len() as u32;

    if page_count == 0 || page_texts.iter().all(|p| p.trim().is_empty()) {
        bail!(
            "PDF '{}' produced no extractable text: it may be a scanned image PDF. \
             Convert it to a text-layer PDF first.",
            filename
        );
    }

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

    let full_text = pages
        .iter()
        .map(|p| p.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");

    Ok(DocumentContent {
        full_text,
        meta: DocumentMeta {
            title,
            author,
            page_count,
        },
        pages,
    })
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
