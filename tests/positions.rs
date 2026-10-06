use std::path::PathBuf;

use kconfig_lsp::analysis::WorldIndex;
use kconfig_lsp::ast::LineIndex;
use kconfig_lsp::{completion, diagnostics, hover, references};
use tower_lsp::lsp_types::{CompletionResponse, HoverContents, Position, Range};

#[test]
fn line_index_counts_utf16_units() {
    // `É` is 2 bytes and 1 unit, `😀` is 4 bytes and 2 units.
    let text = "aÉ😀b\nx";
    let index = LineIndex::new(text);
    for (offset, line_col) in [
        (0, (0, 0)),
        (1, (0, 1)),
        (3, (0, 2)),
        (7, (0, 4)),
        (8, (0, 5)),
        (9, (1, 0)),
    ] {
        assert_eq!(index.line_col(text, offset), line_col, "{offset}");
    }
    for ((line, col), offset) in [
        ((0, 4), 7),
        // Inside `😀`: the next character.
        ((0, 3), 7),
        // Past the end of the line: the end of the line.
        ((0, 99), 8),
        ((1, 0), 9),
        ((1, 5), 10),
    ] {
        assert_eq!(index.offset(text, line, col), offset, "{line}:{col}");
    }
}

const SRC: &str = "config FOO\n\tbool \"CAFÉ 😀\" if FOO\n\tdepends on \"é\" = F";

fn index() -> (WorldIndex, PathBuf) {
    // References need an absolute path to make a file URI.
    let path = std::env::current_dir().unwrap().join("test/Kconfig");
    let mut index = WorldIndex::new();
    index.analyze_file(&path, SRC);
    (index, path)
}

fn range(line: u32, start: u32, end: u32) -> Range {
    Range::new(Position::new(line, start), Position::new(line, end))
}

#[test]
fn positions_after_non_ascii_text_count_utf16_units() {
    let (index, path) = index();
    // `FOO` after the prompt is at units 19 to 22, but at bytes 22 to 25.
    let refs = references::find_references(&index, &path, Position::new(1, 20)).unwrap();
    let ranges: Vec<Range> = refs.iter().map(|l| l.range).collect();
    assert_eq!(ranges, [range(0, 7, 10), range(1, 19, 22)]);

    let Some(HoverContents::Markup(doc)) =
        hover::hover(&index, &path, Position::new(1, 20)).map(|h| h.contents)
    else {
        panic!("no hover");
    };
    assert!(doc.value.starts_with("**FOO**"), "{}", doc.value);

    let undefined: Vec<Range> = diagnostics::collect(&index, &path)
        .into_iter()
        .map(|d| d.range)
        .collect();
    assert_eq!(undefined, [range(2, 18, 19)]);
}

#[test]
fn completion_after_non_ascii_text() {
    let (index, path) = index();
    // After `F`, and after `é`, which is inside `é` if taken as bytes.
    for (col, want) in [(19, vec!["FOO"]), (14, vec![])] {
        let labels: Vec<String> = match completion::complete(&index, &path, Position::new(2, col)) {
            Some(CompletionResponse::Array(items)) => items.into_iter().map(|i| i.label).collect(),
            _ => Vec::new(),
        };
        assert_eq!(labels, want, "{col}");
    }
}
