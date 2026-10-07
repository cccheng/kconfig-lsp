use std::path::Path;

use tower_lsp::lsp_types::*;

use crate::analysis::{FileAnalysis, SymbolDef, WorldIndex};
use crate::ast::{Attribute, Entry, Span};

/// The outline of a file. Menus, choices and `if` blocks hold the entries
/// in them.
pub fn document_symbols(index: &WorldIndex, path: &Path) -> Option<DocumentSymbolResponse> {
    let fa = index.files.get(path)?;
    Some(DocumentSymbolResponse::Nested(entry_symbols(
        fa,
        &fa.file.entries,
    )))
}

/// The definitions in all indexed files whose names contain `query`,
/// ignoring case. An empty query matches all definitions.
#[allow(deprecated)]
pub fn workspace_symbols(index: &WorldIndex, query: &str) -> Vec<SymbolInformation> {
    let query = query.to_lowercase();
    let mut defs: Vec<&SymbolDef> = index
        .definitions
        .values()
        .flatten()
        .filter(|d| d.name.to_lowercase().contains(&query))
        .collect();
    defs.sort_by(|a, b| {
        (&a.name, &a.file, a.name_span.start).cmp(&(&b.name, &b.file, b.name_span.start))
    });
    defs.into_iter()
        .filter_map(|d| {
            let fa = index.files.get(&d.file)?;
            let uri = Url::from_file_path(&d.file).ok()?;
            Some(SymbolInformation {
                name: d.name.clone(),
                kind: SymbolKind::VARIABLE,
                tags: None,
                deprecated: None,
                location: Location::new(uri, range(fa, d.name_span)),
                container_name: None,
            })
        })
        .collect()
}

fn entry_symbols(fa: &FileAnalysis, entries: &[Entry]) -> Vec<DocumentSymbol> {
    entries.iter().filter_map(|e| entry_symbol(fa, e)).collect()
}

fn entry_symbol(fa: &FileAnalysis, entry: &Entry) -> Option<DocumentSymbol> {
    match entry {
        Entry::Config(c) | Entry::MenuConfig(c) | Entry::ConfigDefault(c) => {
            // A parse error can leave the name empty.
            if c.name.is_empty() {
                return None;
            }
            let detail = match entry {
                Entry::ConfigDefault(_) => Some("configdefault".to_string()),
                _ => prompt(&c.attributes),
            };
            Some(symbol(
                fa,
                c.name.clone(),
                detail,
                SymbolKind::VARIABLE,
                (c.span, c.name_span),
                None,
            ))
        }
        Entry::Menu(m) => {
            // LSP rejects a name of only blanks.
            let name = if m.prompt.trim().is_empty() {
                "menu".to_string()
            } else {
                m.prompt.clone()
            };
            let children = entry_symbols(fa, &m.entries);
            Some(symbol(
                fa,
                name,
                None,
                SymbolKind::MODULE,
                (m.span, m.prompt_span),
                Some(children),
            ))
        }
        Entry::Choice(c) => {
            let prompt = prompt(&c.attributes);
            let (name, detail, selection) = match &c.name {
                Some((name, span)) if !name.trim().is_empty() => (name.clone(), prompt, *span),
                _ => {
                    let name = prompt.unwrap_or_else(|| "choice".to_string());
                    // The span starts at `choice`.
                    let keyword = Span::new(c.span.start, c.span.start + "choice".len());
                    (name, None, keyword)
                }
            };
            let children = entry_symbols(fa, &c.entries);
            Some(symbol(
                fa,
                name,
                detail,
                SymbolKind::ENUM,
                (c.span, selection),
                Some(children),
            ))
        }
        Entry::If(i) => {
            let cond = i.condition.span();
            let text = fa.source.get(cond.start..cond.end).unwrap_or("");
            let name = std::iter::once("if")
                .chain(text.split_whitespace())
                .collect::<Vec<_>>()
                .join(" ");
            let children = entry_symbols(fa, &i.entries);
            Some(symbol(
                fa,
                name,
                None,
                SymbolKind::NAMESPACE,
                (i.span, cond),
                Some(children),
            ))
        }
        Entry::Comment(_) | Entry::Source(_) | Entry::MainMenu(_) => None,
    }
}

/// The last prompt of an entry, as analysis takes it.
fn prompt(attrs: &[Attribute]) -> Option<String> {
    attrs
        .iter()
        .filter_map(|a| match a {
            Attribute::Type(t) => t.prompt.as_ref(),
            Attribute::Prompt(p) => Some(p),
            _ => None,
        })
        .next_back()
        .filter(|p| !p.text.trim().is_empty())
        .map(|p| p.text.clone())
}

/// `spans` is the span of the whole entry and the span of its name.
#[allow(deprecated)]
fn symbol(
    fa: &FileAnalysis,
    name: String,
    detail: Option<String>,
    kind: SymbolKind,
    spans: (Span, Span),
    children: Option<Vec<DocumentSymbol>>,
) -> DocumentSymbol {
    DocumentSymbol {
        name,
        detail,
        kind,
        tags: None,
        deprecated: None,
        range: range(fa, spans.0),
        selection_range: range(fa, spans.1),
        children,
    }
}

fn range(fa: &FileAnalysis, span: Span) -> Range {
    let (line, col) = fa.line_index.line_col(&fa.source, span.start);
    let (end_line, end_col) = fa.line_index.line_col(&fa.source, span.end);
    Range::new(Position::new(line, col), Position::new(end_line, end_col))
}
