use std::collections::HashMap;
use std::path::PathBuf;

use kconfig_lsp::analysis::WorldIndex;
use kconfig_lsp::rename::{prepare_rename, rename};
use kconfig_lsp::settings::Settings;
use tower_lsp::lsp_types::{Position, Range, TextEdit, Url};

fn index(source: &str) -> (WorldIndex, PathBuf) {
    let path = std::env::current_dir().unwrap().join("test/Kconfig");
    let mut index = WorldIndex::new();
    index.analyze_file(&path, source);
    (index, path)
}

fn range(line: u32, start: u32, end: u32) -> Range {
    Range::new(Position::new(line, start), Position::new(line, end))
}

fn edit(line: u32, start: u32, end: u32, new_name: &str) -> TextEdit {
    TextEdit::new(range(line, start, end), new_name.to_string())
}

#[test]
fn prepare_definition_and_reference() {
    let (index, path) = index("config FOO\n\tbool\n\tdepends on FOO\n");
    for col in 7..=10 {
        assert_eq!(
            prepare_rename(&index, &path, Position::new(0, col)),
            Some(range(0, 7, 10))
        );
    }
    for col in 12..=15 {
        assert_eq!(
            prepare_rename(&index, &path, Position::new(2, col)),
            Some(range(2, 12, 15))
        );
    }
}

#[test]
fn prepare_rejects_non_symbols() {
    let source = "config FOO\n\tbool \"FOO\"\n\tdefault y\n\tdefault 123\n\tdepends on $(FOO)\n\thelp\n\t  FOO help text\n";
    let (index, path) = index(source);
    for pos in [
        Position::new(0, 2),  // Keyword.
        Position::new(1, 8),  // String.
        Position::new(2, 9),  // y.
        Position::new(3, 10), // Number.
        Position::new(4, 15), // Macro argument.
        Position::new(6, 4),  // Help text.
    ] {
        assert_eq!(prepare_rename(&index, &path, pos), None, "{pos:?}");
        assert_eq!(rename(&index, &path, pos, "BAR").unwrap(), None);
    }
}

#[test]
fn prepare_rejects_a_word_that_is_a_symbol_only_in_another_file() {
    let (mut index, path) = index("if FOO\nendif\n");
    let other = path.with_file_name("Kconfig.other");
    // `FOO` is at the same offsets as in the first file, but in a comment.
    index.analyze_file(&other, "#  FOO\n");
    assert_eq!(prepare_rename(&index, &other, Position::new(0, 4)), None);
    assert_eq!(
        prepare_rename(&index, &path, Position::new(0, 4)),
        Some(range(0, 3, 6))
    );
}

#[test]
fn rename_across_files_and_configdefault() {
    let (mut index, path) = index("config FOO\n\tbool \"FOO\"\n");
    index.settings = Settings {
        zephyr_extensions: true,
        ..Default::default()
    };
    let other = path.with_file_name("Kconfig.other");
    index.analyze_file(
        &other,
        "config OTHER\n\tbool\n\tdepends on FOO\n\tselect FOO\n\tdefault y if !FOO\nconfigdefault FOO\n\tdefault y\n",
    );
    assert_eq!(
        prepare_rename(&index, &other, Position::new(5, 15)),
        Some(range(5, 14, 17))
    );
    let changes = rename(&index, &path, Position::new(0, 8), "BAR")
        .unwrap()
        .unwrap()
        .changes
        .unwrap();
    assert_eq!(
        changes,
        HashMap::from([
            (
                Url::from_file_path(&path).unwrap(),
                vec![edit(0, 7, 10, "BAR")],
            ),
            (
                Url::from_file_path(&other).unwrap(),
                vec![
                    edit(2, 12, 15, "BAR"),
                    edit(3, 8, 11, "BAR"),
                    edit(4, 15, 18, "BAR"),
                    edit(5, 14, 17, "BAR"),
                ],
            ),
        ])
    );
}

#[test]
fn rename_uses_utf16_columns() {
    let (index, path) = index("config FOO\n\tbool \"CAFÉ 😀\" if FOO\n");
    assert_eq!(
        prepare_rename(&index, &path, Position::new(1, 20)),
        Some(range(1, 19, 22))
    );
    let changes = rename(&index, &path, Position::new(1, 20), "BAR")
        .unwrap()
        .unwrap()
        .changes
        .unwrap();
    assert_eq!(
        changes[&Url::from_file_path(&path).unwrap()],
        vec![edit(0, 7, 10, "BAR"), edit(1, 19, 22, "BAR")]
    );
}

#[test]
fn rename_rejects_invalid_and_existing_names() {
    let (index, path) = index("config FOO\n\tdepends on UNKNOWN\nconfig BAR\n\tbool\n");
    let pos = Position::new(0, 8);
    let names = ["", "BAD-NAME", "BAD NAME", "É", "$(FOO)"];
    // Keywords, tristate values and numbers are not symbols.
    let not_symbols = ["if", "config", "bool", "on", "y", "n", "m", "123", "0x1F"];
    for name in names.into_iter().chain(not_symbols) {
        assert_eq!(
            rename(&index, &path, pos, name).unwrap_err(),
            format!("`{name}` is not a valid symbol name")
        );
    }
    for name in ["BAR", "UNKNOWN"] {
        assert_eq!(
            rename(&index, &path, pos, name).unwrap_err(),
            format!("`{name}` is a symbol already")
        );
    }
    assert!(rename(&index, &path, pos, "FOO").unwrap().is_some());
}

#[test]
fn rename_reference_only_symbol() {
    let (index, path) = index("config OTHER\n\tdepends on MISSING\n\tselect MISSING\n");
    assert!(index.get_definitions("MISSING").is_empty());
    let changes = rename(&index, &path, Position::new(1, 14), "FOUND")
        .unwrap()
        .unwrap()
        .changes
        .unwrap();
    assert_eq!(
        changes[&Url::from_file_path(&path).unwrap()],
        vec![edit(1, 12, 19, "FOUND"), edit(2, 8, 15, "FOUND")]
    );
}

#[test]
fn rename_sorts_and_deduplicates_edits() {
    let (mut index, path) = index("config FOO\n\tdepends on FOO\n\tselect FOO\n");
    let definitions = index.definitions.get_mut("FOO").unwrap();
    definitions.push(definitions[0].clone());
    let references = index.references.get_mut("FOO").unwrap();
    references.reverse();
    references.push(references[0].clone());
    let changes = rename(&index, &path, Position::new(0, 8), "BAR")
        .unwrap()
        .unwrap()
        .changes
        .unwrap();
    assert_eq!(
        changes[&Url::from_file_path(&path).unwrap()],
        vec![
            edit(0, 7, 10, "BAR"),
            edit(1, 12, 15, "BAR"),
            edit(2, 8, 11, "BAR"),
        ]
    );
}

#[test]
fn prepare_at_end_of_file() {
    let (index, path) = index("config FOO");
    assert_eq!(
        prepare_rename(&index, &path, Position::new(0, 10)),
        Some(range(0, 7, 10))
    );
}
