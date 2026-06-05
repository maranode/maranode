use crate::extract::DocumentContent;

#[derive(Debug, Clone)]
pub struct RichChunk {
    pub text: String,
    pub page_number: u32,
    pub section: Option<String>,
}

pub fn chunk_document(doc: &DocumentContent, size: usize, overlap: usize) -> Vec<RichChunk> {
    let mut out = Vec::new();
    for page in &doc.pages {
        let section = extract_heading(&page.text);
        for text in chunk_text(&page.text, size, overlap) {
            out.push(RichChunk {
                text,
                page_number: page.number,
                section: section.clone(),
            });
        }
    }
    out
}

pub fn chunk_text(text: &str, size: usize, overlap: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let size = size.max(1);
    let overlap = overlap.min(size.saturating_sub(1));

    if chars.len() <= size {
        let s = text.trim();
        return if s.is_empty() {
            Vec::new()
        } else {
            vec![s.to_string()]
        };
    }

    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let hard_end = (start + size).min(chars.len());
        let end = if hard_end == chars.len() {
            hard_end
        } else {
            find_break(&chars, start, hard_end)
        };

        let piece: String = chars[start..end].iter().collect();
        let trimmed = piece.trim();
        if !trimmed.is_empty() {
            chunks.push(trimmed.to_string());
        }

        if end >= chars.len() {
            break;
        }
        let next = end.saturating_sub(overlap);
        start = if next > start { next } else { end };
    }
    chunks
}

fn extract_heading(text: &str) -> Option<String> {
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        // line that starts with # is a markdown heading
        if t.starts_with('#') {
            return Some(t.trim_start_matches('#').trim().to_string());
        }
        if t.len() <= 60 && !t.ends_with('.') && !t.ends_with(',') {
            return Some(t.to_string());
        }
        break;
    }
    None
}

fn find_break(chars: &[char], start: usize, hard_end: usize) -> usize {
    let window = hard_end - start;
    let min_len = (window * 6) / 10;

    for i in (start + min_len..hard_end).rev() {
        if chars[i] == '\n' && i > start && chars[i - 1] == '\n' {
            return i + 1;
        }
    }
    for i in (start + min_len..hard_end).rev() {
        if matches!(chars[i], '.' | '!' | '?' | '…')
            && chars.get(i + 1).map(|c| c.is_whitespace()).unwrap_or(true)
        {
            return i + 1;
        }
    }
    for i in (start + min_len..hard_end).rev() {
        if chars[i].is_whitespace() {
            return i + 1;
        }
    }
    hard_end
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::DocumentContent;

    #[test]
    fn short_text_is_single_chunk() {
        assert_eq!(chunk_text("hello world", 100, 10), vec!["hello world"]);
    }

    #[test]
    fn empty_text_yields_nothing() {
        assert!(chunk_text("", 100, 10).is_empty());
        assert!(chunk_text("   \n  ", 100, 10).is_empty());
    }

    #[test]
    fn long_text_is_split() {
        let body = "abcdefghij ".repeat(200);
        let out = chunk_text(&body, 500, 50);
        assert!(out.len() > 1);
        for c in &out {
            assert!(c.chars().count() <= 500);
        }
    }

    #[test]
    fn chunk_document_tracks_page_numbers() {
        let doc = DocumentContent::from_plain("Page one content.".into());
        let chunks = chunk_document(&doc, 500, 50);
        assert!(!chunks.is_empty());
        assert_eq!(chunks[0].page_number, 1);
    }

    #[test]
    fn heading_extracted() {
        let text = "# Revenue Overview\n\nRevenue was $10M last year.";
        let h = extract_heading(text);
        assert_eq!(h.as_deref(), Some("Revenue Overview"));
    }
}
