use std::path::Path;

use tower_lsp::lsp_types::*;

use crate::analysis::WorldIndex;

pub fn goto_definition(
    index: &WorldIndex,
    path: &Path,
    pos: Position,
) -> Option<GotoDefinitionResponse> {
    let fa = index.files.get(path)?;
    let offset = fa.line_index.offset(pos.line, pos.character);
    let word = word_at_offset(&fa.source, offset)?;

    // `configdefault` blocks come after the definitions.
    let defs = index
        .get_definitions(&word)
        .iter()
        .map(|d| (&d.file, d.name_span));
    let configdefaults = index
        .get_configdefaults(&word)
        .iter()
        .map(|c| (&c.file, c.name_span));

    let locations: Vec<Location> = defs
        .chain(configdefaults)
        .filter_map(|(file, span)| {
            let target_fa = index.files.get(file)?;
            let (line, col) = target_fa.line_index.line_col(span.start);
            let (end_line, end_col) = target_fa.line_index.line_col(span.end);
            let uri = Url::from_file_path(file).ok()?;
            Some(Location {
                uri,
                range: Range {
                    start: Position::new(line, col),
                    end: Position::new(end_line, end_col),
                },
            })
        })
        .collect();

    if locations.is_empty() {
        None
    } else if locations.len() == 1 {
        Some(GotoDefinitionResponse::Scalar(
            locations.into_iter().next().unwrap(),
        ))
    } else {
        Some(GotoDefinitionResponse::Array(locations))
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
