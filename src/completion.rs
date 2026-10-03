use std::path::Path;

use tower_lsp::lsp_types::*;

use crate::analysis::WorldIndex;
use crate::lexer::{Lexer, TokenKind};

pub fn complete(index: &WorldIndex, path: &Path, pos: Position) -> Option<CompletionResponse> {
    let fa = index.files.get(path)?;
    let offset = fa.line_index.offset(pos.line, pos.character);
    let prefix = prefix_at_offset(&fa.source, offset);

    let mut items: Vec<CompletionItem> = Vec::new();

    let zephyr_keywords: &[&str] = if index.settings.zephyr_extensions {
        ZEPHYR_KEYWORDS
    } else {
        &[]
    };
    for kw in KEYWORDS.iter().chain(zephyr_keywords) {
        if kw.starts_with(&prefix) || prefix.is_empty() {
            items.push(CompletionItem {
                label: kw.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            });
        }
    }

    let offer_symbols = !prefix.is_empty() || is_symbol_position(index, &fa.source, offset);
    for sym in &index.all_symbols {
        if offer_symbols && sym.starts_with(&prefix) {
            items.push(CompletionItem {
                label: sym.clone(),
                kind: Some(CompletionItemKind::CONSTANT),
                detail: index
                    .get_definitions(sym)
                    .first()
                    .and_then(|d| d.prompt.clone()),
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

/// Whether a symbol can start at `offset`, judged by the last token before
/// it on the same line.
fn is_symbol_position(index: &WorldIndex, source: &str, offset: usize) -> bool {
    let line_start = source[..offset].rfind('\n').map_or(0, |p| p + 1);
    let tokens = Lexer::new(&source[line_start..offset], &index.settings).tokenize();
    let kinds: Vec<&TokenKind> = tokens
        .iter()
        .map(|t| &t.kind)
        .filter(|k| !matches!(k, TokenKind::Newline | TokenKind::Eof))
        .collect();
    let (Some(first), Some(last)) = (kinds.first(), kinds.last()) else {
        return false;
    };
    // Help text and other prose start with a plain word.
    if matches!(
        first,
        TokenKind::Ident(_) | TokenKind::StringLit(_) | TokenKind::Macro(_)
    ) {
        return false;
    }
    matches!(
        last,
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
const ZEPHYR_KEYWORDS: &[&str] = &["configdefault", "def_int", "def_hex", "def_string"];
