use std::path::Path;

use tower_lsp::lsp_types::*;

use crate::analysis::WorldIndex;

pub fn complete(index: &WorldIndex, path: &Path, pos: Position) -> Option<CompletionResponse> {
    let fa = index.files.get(path)?;
    let offset = fa.line_index.offset(pos.line, pos.character);
    let prefix = prefix_at_offset(&fa.source, offset);

    let mut items: Vec<CompletionItem> = Vec::new();

    for kw in KEYWORDS {
        if kw.starts_with(&prefix) || prefix.is_empty() {
            items.push(CompletionItem {
                label: kw.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            });
        }
    }

    for sym in &index.all_symbols {
        if sym.starts_with(&prefix) || (prefix.is_empty() && is_symbol_position(&fa.source, offset))
        {
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

fn is_symbol_position(source: &str, offset: usize) -> bool {
    let line_start = source[..offset].rfind('\n').map_or(0, |p| p + 1);
    let before = source[line_start..offset].trim_start();
    matches!(
        before,
        "depends on " | "select " | "imply " | "default " | "def_bool " | "def_tristate "
    ) || before.starts_with("depends on ")
        || before.starts_with("select ")
        || before.starts_with("imply ")
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
