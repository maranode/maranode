//! a small syntax-aware symbol outline and search. it scans definition lines and
//! lists the functions, types and other named definitions with their line numbers.
//! heuristic, not a full parser, but language-aware through the file extension.

use crate::chunk::{lang_for, CodeLang};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Impl,
    Class,
    Type,
    Module,
}

impl SymbolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SymbolKind::Function => "fn",
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::Trait => "trait",
            SymbolKind::Impl => "impl",
            SymbolKind::Class => "class",
            SymbolKind::Type => "type",
            SymbolKind::Module => "module",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Symbol {
    pub kind: SymbolKind,
    pub name: String,
    /// 1-based line number of the definition
    pub line: usize,
}

/// pick the language from a file name; convenience wrapper over the chunker.
pub fn language_of(source_name: &str) -> Option<CodeLang> {
    lang_for(source_name)
}

/// list the definitions in a source file, in source order.
pub fn outline(source: &str, lang: CodeLang) -> Vec<Symbol> {
    let mut out = Vec::new();
    for (i, raw) in source.lines().enumerate() {
        let line = raw.trim_start();
        if line.is_empty()
            || line.starts_with("//")
            || line.starts_with('#')
            || line.starts_with('*')
            || line.starts_with("/*")
        {
            continue;
        }
        if let Some((kind, name)) = parse_def(line, lang) {
            out.push(Symbol {
                kind,
                name,
                line: i + 1,
            });
        }
    }
    out
}

/// outline filtered to symbols whose name contains `query` (case-insensitive).
pub fn search(source: &str, lang: CodeLang, query: &str) -> Vec<Symbol> {
    let q = query.to_lowercase();
    outline(source, lang)
        .into_iter()
        .filter(|s| s.name.to_lowercase().contains(&q))
        .collect()
}

fn parse_def(line: &str, lang: CodeLang) -> Option<(SymbolKind, String)> {
    let toks = ident_tokens(line);
    if toks.is_empty() {
        return None;
    }

    match lang {
        CodeLang::Python => {
            let mut idx = 0;
            if toks[idx] == "async" {
                idx += 1;
            }
            match toks.get(idx).copied() {
                Some("def") => name_at(&toks, idx + 1).map(|n| (SymbolKind::Function, n)),
                Some("class") => name_at(&toks, idx + 1).map(|n| (SymbolKind::Class, n)),
                _ => None,
            }
        }
        CodeLang::Braces => {
            const MODS: &[&str] = &[
                "pub", "export", "default", "async", "static", "public", "private", "protected",
                "final", "abstract", "unsafe", "open", "override", "inline", "const", "extern",
                "virtual", "internal", "sealed",
            ];
            let mut idx = 0;
            while idx < toks.len() && MODS.contains(&toks[idx]) {
                idx += 1;
            }
            let kw = toks.get(idx).copied()?;
            let kind = match kw {
                "fn" | "function" | "func" | "fun" => SymbolKind::Function,
                "struct" => SymbolKind::Struct,
                "enum" => SymbolKind::Enum,
                "trait" | "interface" | "protocol" => SymbolKind::Trait,
                "impl" | "extension" => SymbolKind::Impl,
                "class" => SymbolKind::Class,
                "type" | "typedef" => SymbolKind::Type,
                "mod" | "module" | "namespace" | "package" => SymbolKind::Module,
                _ => return None,
            };
            // for `impl Trait for Type` name the concrete type, else the token after the keyword
            let name = if kind == SymbolKind::Impl {
                match toks.iter().position(|t| *t == "for") {
                    Some(p) => name_at(&toks, p + 1),
                    None => name_at(&toks, idx + 1),
                }
            } else {
                name_at(&toks, idx + 1)
            };
            name.map(|n| (kind, n))
        }
    }
}

fn name_at(toks: &[&str], idx: usize) -> Option<String> {
    toks.get(idx)
        .filter(|t| is_ident_name(t))
        .map(|t| t.to_string())
}

fn is_ident_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' => {}
        _ => return false,
    }
    s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// split a line into identifier-ish tokens in order, dropping punctuation.
fn ident_tokens(line: &str) -> Vec<&str> {
    line.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_outline() {
        let src = "\
use std::io;

pub struct Server { port: u16 }

pub async fn start(cfg: Config) -> Result<()> {
    Ok(())
}

impl Server {
    fn new() -> Self { Server { port: 0 } }
}

enum State { On, Off }

impl Drop for Server {
    fn drop(&mut self) {}
}
";
        let syms = outline(src, CodeLang::Braces);
        let names: Vec<_> = syms.iter().map(|s| (s.kind, s.name.as_str())).collect();
        assert!(names.contains(&(SymbolKind::Struct, "Server")));
        assert!(names.contains(&(SymbolKind::Function, "start")));
        assert!(names.contains(&(SymbolKind::Function, "new")));
        assert!(names.contains(&(SymbolKind::Enum, "State")));
        // `impl Drop for Server` names the concrete type
        assert!(names.contains(&(SymbolKind::Impl, "Server")));
    }

    #[test]
    fn python_outline_includes_methods() {
        let src = "\
import os

class Service:
    def run(self):
        return 1

async def main():
    pass
";
        let syms = outline(src, CodeLang::Python);
        let names: Vec<_> = syms.iter().map(|s| (s.kind, s.name.as_str())).collect();
        assert!(names.contains(&(SymbolKind::Class, "Service")));
        assert!(names.contains(&(SymbolKind::Function, "run")));
        assert!(names.contains(&(SymbolKind::Function, "main")));
    }

    #[test]
    fn js_functions_and_classes() {
        let src = "\
export function handler(req) {}
class Widget {}
export default function () {}
const x = pattern.exec(s);
";
        let syms = outline(src, CodeLang::Braces);
        let names: Vec<_> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"handler"));
        assert!(names.contains(&"Widget"));
        // anonymous default export has no name, and a const assignment is not a definition
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn comments_are_skipped() {
        let src = "// fn ghost() {}\n#[allow(dead_code)]\nfn real() {}\n";
        let syms = outline(src, CodeLang::Braces);
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].name, "real");
        assert_eq!(syms[0].line, 3);
    }

    #[test]
    fn search_filters_by_name() {
        let src = "fn load_config() {}\nfn save_config() {}\nfn run() {}\n";
        let hits = search(src, CodeLang::Braces, "config");
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().all(|s| s.name.contains("config")));
    }
}
