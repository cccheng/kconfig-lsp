use std::collections::HashMap;
use std::path::Path;

use tower_lsp::lsp_types::*;

use crate::analysis::{FileAnalysis, WorldIndex, is_numeric_literal, is_tristate_literal};
use crate::ast::Span;
use crate::lexer::{Lexer, Token, TokenKind};

/// The range of the symbol at `pos`, if `pos` is on a definition or a
/// reference of the symbol.
pub fn prepare_rename(index: &WorldIndex, path: &Path, pos: Position) -> Option<Range> {
    symbol_at_position(index, path, pos).map(|(_, range)| range)
}

/// Renames the symbol at `pos` in all indexed files.
pub fn rename(
    index: &WorldIndex,
    path: &Path,
    pos: Position,
    new_name: &str,
) -> Result<Option<WorkspaceEdit>, String> {
    let Some((old_name, _)) = symbol_at_position(index, path, pos) else {
        return Ok(None);
    };
    if !is_symbol_name(index, new_name) {
        return Err(format!("`{new_name}` is not a valid symbol name"));
    }
    if new_name != old_name
        && (!index.get_definitions(new_name).is_empty()
            || !index.get_references(new_name).is_empty())
    {
        return Err(format!("`{new_name}` is a symbol already"));
    }

    let occurrences = index
        .get_definitions(&old_name)
        .iter()
        .map(|d| (d.file.as_path(), d.name_span))
        .chain(
            index
                .get_references(&old_name)
                .iter()
                .map(|r| (r.file.as_path(), r.span)),
        );
    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    for (file, span) in occurrences {
        if let (Some(fa), Ok(uri)) = (index.files.get(file), Url::from_file_path(file)) {
            changes.entry(uri).or_default().push(TextEdit {
                range: span_range(fa, span),
                new_text: new_name.to_string(),
            });
        }
    }
    for edits in changes.values_mut() {
        edits.sort_by_key(|edit| {
            let range = edit.range;
            (
                range.start.line,
                range.start.character,
                range.end.line,
                range.end.character,
            )
        });
        edits.dedup();
    }
    Ok(Some(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    }))
}

/// Whether `name` can name a symbol: one word that is not a keyword,
/// `y`, `n`, `m` or a number.
fn is_symbol_name(index: &WorldIndex, name: &str) -> bool {
    let tokens = Lexer::new(name, &index.settings).tokenize();
    let one_word = matches!(
        &tokens[..],
        [Token { kind: TokenKind::Ident(word), .. }, _] if word == name
    );
    one_word
        && name.bytes().all(is_word_char)
        && !is_tristate_literal(name)
        && !is_numeric_literal(name)
}

/// The name and range of the symbol at `pos`. The word at `pos` must be
/// a definition or a reference in this file, so that strings, help text
/// and macros give no symbol.
fn symbol_at_position(index: &WorldIndex, path: &Path, pos: Position) -> Option<(String, Range)> {
    let fa = index.files.get(path)?;
    let offset = fa.line_index.offset(&fa.source, pos.line, pos.character);
    let word = word_at_offset(&fa.source, offset)?;
    let span = index
        .get_definitions(&word)
        .iter()
        .map(|d| (d.file.as_path(), d.name_span))
        .chain(
            index
                .get_references(&word)
                .iter()
                .map(|r| (r.file.as_path(), r.span)),
        )
        .find_map(|(file, span)| {
            (file == path && span.start <= offset && offset <= span.end).then_some(span)
        })?;
    Some((word, span_range(fa, span)))
}

fn span_range(fa: &FileAnalysis, span: Span) -> Range {
    let (line, col) = fa.line_index.line_col(&fa.source, span.start);
    let (end_line, end_col) = fa.line_index.line_col(&fa.source, span.end);
    Range::new(Position::new(line, col), Position::new(end_line, end_col))
}

fn word_at_offset(source: &str, offset: usize) -> Option<String> {
    let bytes = source.as_bytes();
    if offset > bytes.len() {
        return None;
    }
    let mut start = offset;
    while start > 0 && is_word_char(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = offset;
    while end < bytes.len() && is_word_char(bytes[end]) {
        end += 1;
    }
    if start == end {
        return None;
    }
    Some(source[start..end].to_string())
}

fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}
