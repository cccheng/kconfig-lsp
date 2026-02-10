use std::path::Path;

use tower_lsp::lsp_types::*;

use crate::analysis::WorldIndex;

pub fn find_references(index: &WorldIndex, path: &Path, pos: Position) -> Option<Vec<Location>> {
    let fa = index.files.get(path)?;
    let offset = fa.line_index.offset(pos.line, pos.character);
    let word = word_at_offset(&fa.source, offset)?;

    let mut locations: Vec<Location> = Vec::new();

    for d in index.get_definitions(&word) {
        if let Some(target_fa) = index.files.get(&d.file) {
            let (line, col) = target_fa.line_index.line_col(d.name_span.start);
            let (end_line, end_col) = target_fa.line_index.line_col(d.name_span.end);
            if let Ok(uri) = Url::from_file_path(&d.file) {
                locations.push(Location {
                    uri,
                    range: Range {
                        start: Position::new(line, col),
                        end: Position::new(end_line, end_col),
                    },
                });
            }
        }
    }

    for r in index.get_references(&word) {
        if let Some(target_fa) = index.files.get(&r.file) {
            let (line, col) = target_fa.line_index.line_col(r.span.start);
            let (end_line, end_col) = target_fa.line_index.line_col(r.span.end);
            if let Ok(uri) = Url::from_file_path(&r.file) {
                locations.push(Location {
                    uri,
                    range: Range {
                        start: Position::new(line, col),
                        end: Position::new(end_line, end_col),
                    },
                });
            }
        }
    }

    if locations.is_empty() {
        None
    } else {
        Some(locations)
    }
}

fn word_at_offset(source: &str, offset: usize) -> Option<String> {
    let bytes = source.as_bytes();
    if offset >= bytes.len() {
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
