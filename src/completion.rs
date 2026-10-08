use std::path::Path;

use tower_lsp::lsp_types::*;

use crate::analysis::{FileAnalysis, WorldIndex};
use crate::ast::{Attribute, Entry};
use crate::lexer::{Lexer, TokenKind};

pub fn complete(index: &WorldIndex, path: &Path, pos: Position) -> Option<CompletionResponse> {
    let fa = index.files.get(path)?;
    let offset = fa.line_index.offset(&fa.source, pos.line, pos.character);
    let prefix = prefix_at_offset(&fa.source, offset);

    let mut items: Vec<CompletionItem> = Vec::new();

    let zephyr_keywords: &[&str] = if index.settings.zephyr_extensions {
        ZEPHYR_KEYWORDS
    } else {
        &[]
    };
    let offer_keywords = !prefix.is_empty() || is_keyword_position(fa, offset);
    for kw in KEYWORDS.iter().chain(zephyr_keywords) {
        if offer_keywords && kw.starts_with(&prefix) {
            items.push(CompletionItem {
                label: kw.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            });
        }
    }

    let offer_symbols = !prefix.is_empty() || is_symbol_position(index, fa, offset);
    // A symbol leaves `definitions` with its last definition, so each key
    // has one.
    for (sym, defs) in &index.definitions {
        if offer_symbols && sym.starts_with(&prefix) {
            items.push(CompletionItem {
                label: sym.clone(),
                kind: Some(CompletionItemKind::CONSTANT),
                detail: defs.first().and_then(|d| d.prompt.clone()),
                ..Default::default()
            });
        }
    }

    if items.is_empty() {
        None
    } else {
        Some(CompletionResponse::Array(items))
    }
}

fn prefix_at_offset(source: &str, offset: usize) -> String {
    let bytes = source.as_bytes();
    let mut start = offset;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }
    source[start..offset].to_string()
}

/// Whether a keyword can start at `offset`: only blanks precede it on its
/// line, outside help text.
fn is_keyword_position(fa: &FileAnalysis, offset: usize) -> bool {
    let line_start = fa.source[..offset].rfind('\n').map_or(0, |p| p + 1);
    fa.source[line_start..offset]
        .bytes()
        .all(|b| b == b' ' || b == b'\t')
        && !in_help_text(&fa.file.entries, offset)
}

/// Whether a symbol can start at `offset`, judged by the last token before
/// it on the same line.
fn is_symbol_position(index: &WorldIndex, fa: &FileAnalysis, offset: usize) -> bool {
    if in_help_text(&fa.file.entries, offset) {
        return false;
    }
    let source = &fa.source;
    let line_start = source[..offset].rfind('\n').map_or(0, |p| p + 1);
    let mut tokens = Lexer::new(&source[line_start..offset], &index.settings).tokenize();
    tokens.pop(); // Eof
    let (Some(first), Some(last)) = (tokens.first(), tokens.last()) else {
        return false;
    };
    // Help text and other prose start with a plain word.
    if matches!(
        first.kind,
        TokenKind::Ident(_) | TokenKind::StringLit(_) | TokenKind::Macro(_)
    ) {
        return false;
    }
    matches!(
        last.kind,
        TokenKind::On
            | TokenKind::Select
            | TokenKind::Imply
            | TokenKind::Default
            | TokenKind::DefType(_)
            | TokenKind::Range
            | TokenKind::If
            | TokenKind::ConfigDefault
            | TokenKind::Not
            | TokenKind::And
            | TokenKind::Or
            | TokenKind::OpenParen
            | TokenKind::Eq
            | TokenKind::NotEq
            | TokenKind::Less
            | TokenKind::Greater
            | TokenKind::LessEq
            | TokenKind::GreaterEq
    )
}

fn in_help_text(entries: &[Entry], offset: usize) -> bool {
    entries.iter().any(|entry| {
        let (attributes, children): (&[Attribute], &[Entry]) = match entry {
            Entry::Config(c) | Entry::ConfigDefault(c) | Entry::MenuConfig(c) => {
                (&c.attributes, &[])
            }
            Entry::Choice(c) => (&c.attributes, &c.entries),
            Entry::Comment(c) => (&c.attributes, &[]),
            Entry::Menu(m) => (&m.attributes, &m.entries),
            Entry::If(i) => (&[], &i.entries),
            Entry::Source(_) | Entry::MainMenu(_) => (&[], &[]),
        };
        attributes.iter().any(
            // A cursor right before `help` is not in the text.
            |a| matches!(a, Attribute::Help(h) if h.span.start < offset && offset <= h.span.end),
        ) || in_help_text(children, offset)
    })
}

const KEYWORDS: &[&str] = &[
    "config",
    "menuconfig",
    "choice",
    "endchoice",
    "comment",
    "menu",
    "endmenu",
    "if",
    "endif",
    "source",
    "mainmenu",
    "bool",
    "tristate",
    "string",
    "hex",
    "int",
    "prompt",
    "default",
    "def_bool",
    "def_tristate",
    "depends",
    "select",
    "imply",
    "visible",
    "range",
    "help",
    "modules",
    "transitional",
    "optional",
];

/// Keywords only recognized with `zephyr_extensions` enabled.
const ZEPHYR_KEYWORDS: &[&str] = &[
    "configdefault",
    "def_int",
    "def_hex",
    "def_string",
    "rsource",
    "osource",
    "orsource",
];
