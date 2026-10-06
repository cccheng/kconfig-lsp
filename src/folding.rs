use std::path::Path;

use tower_lsp::lsp_types::{FoldingRange, FoldingRangeKind};

use crate::analysis::{FileAnalysis, WorldIndex};
use crate::ast::{Attribute, Entry, Span};

/// Folding ranges for menus, choices, `if` blocks and help text.
pub fn folding_ranges(index: &WorldIndex, path: &Path) -> Option<Vec<FoldingRange>> {
    let fa = index.files.get(path)?;
    let mut ranges = Vec::new();
    collect(fa, &fa.file.entries, &mut ranges);
    Some(ranges)
}

fn collect(fa: &FileAnalysis, entries: &[Entry], out: &mut Vec<FoldingRange>) {
    for entry in entries {
        let (attrs, block) = match entry {
            Entry::Config(c) | Entry::MenuConfig(c) | Entry::ConfigDefault(c) => {
                (&c.attributes[..], None)
            }
            Entry::Choice(c) => (&c.attributes[..], Some((c.span, "endchoice", &c.entries))),
            Entry::Menu(m) => (&m.attributes[..], Some((m.span, "endmenu", &m.entries))),
            Entry::If(i) => (&[][..], Some((i.span, "endif", &i.entries))),
            Entry::Comment(_) | Entry::Source(_) | Entry::MainMenu(_) => continue,
        };
        if let Some((span, end_keyword, _)) = block {
            let text = &fa.source[..span.end];
            let closed = text
                .strip_suffix(end_keyword)
                .is_some_and(|rest| rest.ends_with(char::is_whitespace));
            let (start, end) = if closed {
                // Keep the end keyword line visible.
                let (start, end) = lines(fa, span);
                (start, end.saturating_sub(1))
            } else {
                // Without its end keyword, the block runs to the end of the file.
                lines(fa, Span::new(span.start, text.trim_end().len()))
            };
            push(start, end, None, out);
        }
        for attr in attrs {
            if let Attribute::Help(h) = attr {
                let (start, end) = lines(fa, h.span);
                push(start, end, Some(FoldingRangeKind::Comment), out);
            }
        }
        if let Some((_, _, entries)) = block {
            collect(fa, entries, out);
        }
    }
}

fn lines(fa: &FileAnalysis, span: Span) -> (u32, u32) {
    let (start, _) = fa.line_index.line_col(&fa.source, span.start);
    let (end, _) = fa.line_index.line_col(&fa.source, span.end);
    (start, end)
}

fn push(start: u32, end: u32, kind: Option<FoldingRangeKind>, out: &mut Vec<FoldingRange>) {
    if end > start {
        out.push(FoldingRange {
            start_line: start,
            end_line: end,
            kind,
            ..Default::default()
        });
    }
}
