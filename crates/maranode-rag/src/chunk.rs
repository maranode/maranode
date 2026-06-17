use crate::extract::DocumentContent;

#[derive(Debug, Clone)]
pub struct RichChunk {
    pub text: String,
    pub page_number: u32,
    pub section: Option<String>,
}

pub fn chunk_document(
    doc: &DocumentContent,
    source: &str,
    size: usize,
    overlap: usize,
) -> Vec<RichChunk> {
    if let Some(lang) = lang_for(source) {
        let coded = chunk_code(&doc.full_text, lang, size, overlap);
        if !coded.is_empty() {
            return coded;
        }
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeLang {
    Braces,
    Python,
}

pub fn lang_for(source: &str) -> Option<CodeLang> {
    let ext = std::path::Path::new(source)
        .extension()
        .and_then(|e| e.to_str())?
        .to_lowercase();
    match ext.as_str() {
        "py" | "pyi" => Some(CodeLang::Python),
        "rs" | "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "go" | "java" | "c" | "h" | "cc"
        | "cpp" | "cxx" | "hpp" | "hh" | "hxx" | "cs" | "kt" | "kts" | "swift" | "scala" | "php"
        | "dart" | "proto" => Some(CodeLang::Braces),
        _ => None,
    }
}

pub fn chunk_code(text: &str, lang: CodeLang, size: usize, overlap: usize) -> Vec<RichChunk> {
    let units = match lang {
        CodeLang::Braces => brace_units(text),
        CodeLang::Python => python_units(text),
    };

    let mut out: Vec<RichChunk> = Vec::new();
    let mut buf = String::new();
    let mut label: Option<String> = None;

    for unit in units {
        let unit_len = unit.chars().count();

        if unit_len > size {
            if !buf.trim().is_empty() {
                out.push(mk_chunk(&buf, label.take()));
            }
            buf.clear();
            let unit_label = symbol_label(&unit, lang);
            for piece in chunk_text(&unit, size, overlap) {
                out.push(RichChunk {
                    text: piece,
                    page_number: 1,
                    section: unit_label.clone(),
                });
            }
            continue;
        }

        if !buf.is_empty() && buf.chars().count() + unit_len > size {
            out.push(mk_chunk(&buf, label.take()));
            buf.clear();
        }
        if label.is_none() {
            label = symbol_label(&unit, lang);
        }
        buf.push_str(&unit);
    }

    if !buf.trim().is_empty() {
        out.push(mk_chunk(&buf, label.take()));
    }
    out
}

fn mk_chunk(buf: &str, section: Option<String>) -> RichChunk {
    RichChunk {
        text: buf.trim().to_string(),
        page_number: 1,
        section,
    }
}

fn push_unit(units: &mut Vec<String>, cur: &mut String) {
    if cur.trim().is_empty() {
        cur.clear();
    } else {
        units.push(std::mem::take(cur));
    }
}

fn brace_delta(line: &str) -> i32 {
    let mut depth = 0i32;
    let mut in_str = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if in_str {
            match c {
                '\\' => {
                    chars.next();
                }
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '/' if chars.peek() == Some(&'/') => break,
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
    }
    depth
}

fn brace_units(text: &str) -> Vec<String> {
    let mut units = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;

    for line in text.lines() {
        cur.push_str(line);
        cur.push('\n');
        depth += brace_delta(line);
        if depth < 0 {
            depth = 0;
        }
        let trimmed = line.trim_end();
        if depth == 0 && (trimmed.ends_with('}') || trimmed.ends_with(';')) {
            push_unit(&mut units, &mut cur);
        }
    }
    push_unit(&mut units, &mut cur);
    units
}

fn python_units(text: &str) -> Vec<String> {
    let mut units = Vec::new();
    let mut cur = String::new();
    let mut prev_decorator = false;

    for line in text.lines() {
        if py_top_def(line) && !prev_decorator && !cur.trim().is_empty() {
            push_unit(&mut units, &mut cur);
        }
        cur.push_str(line);
        cur.push('\n');
        prev_decorator = !line.starts_with(char::is_whitespace) && line.trim_start().starts_with('@');
    }
    push_unit(&mut units, &mut cur);
    units
}

fn py_top_def(line: &str) -> bool {
    if line.starts_with(char::is_whitespace) {
        return false;
    }
    let t = line.trim_start();
    t.starts_with("def ")
        || t.starts_with("class ")
        || t.starts_with("async def ")
        || t.starts_with('@')
}

fn symbol_label(unit: &str, lang: CodeLang) -> Option<String> {
    for line in unit.lines().take(10) {
        let t = line.trim();
        if t.is_empty() || t.starts_with("//") || t.starts_with("/*") || t.starts_with('*') {
            continue;
        }
        if let Some(name) = def_name(t, lang) {
            return Some(name);
        }
    }
    None
}

fn def_name(line: &str, lang: CodeLang) -> Option<String> {
    let keywords: &[&str] = match lang {
        CodeLang::Python => &["def", "class"],
        CodeLang::Braces => &[
            "fn", "function", "func", "class", "struct", "enum", "trait", "interface", "type",
            "mod", "impl", "const", "let", "var", "val",
        ],
    };
    let tokens: Vec<&str> = line
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|s| !s.is_empty())
        .collect();
    for pair in tokens.windows(2) {
        if keywords.contains(&pair[0]) && is_ident(pair[1]) {
            return Some(pair[1].to_string());
        }
    }
    None
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' => {}
        _ => return false,
    }
    s.chars().all(|c| c.is_alphanumeric() || c == '_')
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
        let chunks = chunk_document(&doc, "notes.txt", 500, 50);
        assert!(!chunks.is_empty());
        assert_eq!(chunks[0].page_number, 1);
    }

    #[test]
    fn heading_extracted() {
        let text = "# Revenue Overview\n\nRevenue was $10M last year.";
        let h = extract_heading(text);
        assert_eq!(h.as_deref(), Some("Revenue Overview"));
    }

    #[test]
    fn language_detected_from_extension() {
        assert_eq!(lang_for("src/main.rs"), Some(CodeLang::Braces));
        assert_eq!(lang_for("app.py"), Some(CodeLang::Python));
        assert_eq!(lang_for("a/b/server.ts"), Some(CodeLang::Braces));
        assert_eq!(lang_for("readme.md"), None);
        assert_eq!(lang_for("noext"), None);
    }

    #[test]
    fn rust_splits_into_functions() {
        let src = "\
use std::io;

fn alpha(x: i32) -> i32 {
    x + 1
}

fn beta() {
    println!(\"hi\");
}
";
        let chunks = chunk_code(src, CodeLang::Braces, 80, 10);
        let labels: Vec<_> = chunks.iter().filter_map(|c| c.section.clone()).collect();
        assert!(labels.contains(&"alpha".to_string()), "labels: {labels:?}");
        assert!(labels.contains(&"beta".to_string()), "labels: {labels:?}");
        // each function stays whole, not split mid-body
        assert!(chunks.iter().any(|c| c.text.contains("x + 1") && c.text.contains("fn alpha")));
    }

    #[test]
    fn python_splits_at_def_and_keeps_decorator() {
        let src = "\
import os

@app.route('/')
def home():
    return 'ok'

class Service:
    def run(self):
        return 1
";
        let chunks = chunk_code(src, CodeLang::Python, 80, 10);
        let home = chunks.iter().find(|c| c.text.contains("def home"));
        assert!(home.is_some());
        // the decorator rides with its function
        assert!(home.unwrap().text.contains("@app.route"));
        assert!(chunks.iter().any(|c| c.section.as_deref() == Some("Service")));
    }

    #[test]
    fn oversized_unit_falls_back_but_keeps_label() {
        let body = "    let _ = 1;\n".repeat(60);
        let src = format!("fn big() {{\n{body}}}\n");
        let chunks = chunk_code(&src, CodeLang::Braces, 100, 20);
        assert!(chunks.len() > 1, "a large function should be split");
        assert!(chunks.iter().all(|c| c.section.as_deref() == Some("big")));
    }

    #[test]
    fn code_path_used_via_document() {
        let doc = DocumentContent::from_plain("fn only() -> u8 { 7 }\n".into());
        let chunks = chunk_document(&doc, "lib.rs", 200, 20);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].section.as_deref(), Some("only"));
    }
}
