use std::path::Path;

use kconfig_lsp::analysis::WorldIndex;
use kconfig_lsp::completion;
use kconfig_lsp::settings::Settings;
use tower_lsp::lsp_types::{CompletionItemKind, CompletionResponse, Position};

const DEFS: &str = "config FOO\n\tbool \"foo\"\n\nconfig BAR\n\tint \"bar\"\n\n";

/// Symbols offered with the cursor at the end of `line`, appended to `DEFS`.
fn symbol_completions(settings: Settings, line: &str) -> Vec<String> {
    let mut index = WorldIndex::new();
    index.settings = settings;
    let path = Path::new("test/Kconfig");
    let src = format!("{DEFS}{line}");
    index.analyze_file(path, &src);
    let pos = Position::new(DEFS.lines().count() as u32, line.len() as u32);
    match completion::complete(&index, path, pos) {
        Some(CompletionResponse::Array(items)) => items
            .into_iter()
            .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
            .map(|i| i.label)
            .collect(),
        _ => Vec::new(),
    }
}

fn zephyr() -> Settings {
    Settings {
        zephyr_extensions: true,
    }
}

#[test]
fn symbols_offered_where_an_expression_starts() {
    for line in [
        "\tdepends on ",
        "\tdepends on FOO && ",
        "\tdepends on !",
        "\tdepends on (",
        "\tdepends on BAR = ",
        "\tselect ",
        "\timply ",
        "\tdefault ",
        "\tdef_bool ",
        "\tdef_tristate ",
        "\trange ",
        "\tdefault y if ",
        "\tbool \"prompt\" if ",
        "\tvisible if ",
        "if ",
    ] {
        let mut syms = symbol_completions(Settings::default(), line);
        syms.sort();
        assert_eq!(syms, ["BAR", "FOO"], "line {line:?}");
    }
}

#[test]
fn no_symbols_without_prefix_elsewhere() {
    for line in [
        "",
        "\t",
        "config ",
        "\tbool ",
        "\tbool \"use if ",
        "\tdepends on FOO ",
        "\tselect FOO ",
        "\t  Say Y here if ",
        "# depends on ",
        "\tdepends on FOO # if ",
        "\tdef_int ",
        "configdefault ",
    ] {
        assert!(
            symbol_completions(Settings::default(), line).is_empty(),
            "line {line:?}"
        );
    }
}

#[test]
fn prefix_still_matches_anywhere() {
    assert_eq!(symbol_completions(Settings::default(), "\tF"), ["FOO"]);
}

#[test]
fn zephyr_keywords_start_a_symbol_only_when_extension_enabled() {
    for line in [
        "\tdef_int ",
        "\tdef_hex ",
        "\tdef_string ",
        "configdefault ",
    ] {
        let mut syms = symbol_completions(zephyr(), line);
        syms.sort();
        assert_eq!(syms, ["BAR", "FOO"], "line {line:?}");
        assert!(
            symbol_completions(Settings::default(), line).is_empty(),
            "line {line:?}"
        );
    }
}
