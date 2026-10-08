use std::path::Path;

use tower_lsp::lsp_types::{DocumentLink, Position, Range, Url};

use crate::analysis::WorldIndex;
use crate::ast::Span;
use crate::sources;

/// Links from the paths of `source` statements to the files that they
/// name. A path that names no file, or more than one file, gets no link.
pub fn document_links(index: &WorldIndex, path: &Path) -> Option<Vec<DocumentLink>> {
    let fa = index.files.get(path)?;
    let root = index.root.as_deref();
    let links = sources::source_entries(&fa.file)
        .into_iter()
        .filter_map(|entry| {
            let [file] = &sources::resolve(entry, path, &fa.file.variables, root)[..] else {
                return None;
            };
            let target = Url::from_file_path(file).ok()?;
            let span = unquote(&fa.source, entry.path_span);
            let (start_line, start_col) = fa.line_index.line_col(&fa.source, span.start);
            let (end_line, end_col) = fa.line_index.line_col(&fa.source, span.end);
            Some(DocumentLink {
                range: Range::new(
                    Position::new(start_line, start_col),
                    Position::new(end_line, end_col),
                ),
                target: Some(target),
                tooltip: None,
                data: None,
            })
        })
        .collect();
    Some(links)
}

/// The span of a path without its quotes.
fn unquote(source: &str, span: Span) -> Span {
    let text = &source[span.start..span.end];
    let quoted = text.len() >= 2
        && (text.starts_with('"') && text.ends_with('"')
            || text.starts_with('\'') && text.ends_with('\''));
    if quoted {
        Span::new(span.start + 1, span.end - 1)
    } else {
        span
    }
}
