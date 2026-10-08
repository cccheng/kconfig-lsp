use std::path::Path;

use kconfig_lsp::analysis::WorldIndex;
use kconfig_lsp::completion;
use kconfig_lsp::settings::Settings;
use tower_lsp::lsp_types::{CompletionItemKind, CompletionResponse, Position};

const DEFS: &str = "config FOO\n\tbool \"foo\"\n\nconfig BAR\n\tint \"bar\"\n\n";

/// Labels of `kind` offered with the cursor between `before` and `after`,
/// appended to `DEFS`.
fn completions(
    settings: Settings,
    before: &str,
    after: &str,
    kind: CompletionItemKind,
) -> Vec<String> {
    let mut index = WorldIndex::new();
    index.settings = settings;
    let path = Path::new("test/Kconfig");
    let src = format!("{DEFS}{before}{after}");
    index.analyze_file(path, &src);
    let cursor = DEFS.len() + before.len();
    let line_start = src[..cursor].rfind('\n').map_or(0, |p| p + 1);
    let pos = Position::new(
        src[..cursor].matches('\n').count() as u32,
        (cursor - line_start) as u32,
    );
    match completion::complete(&index, path, pos) {
        Some(CompletionResponse::Array(items)) => items
            .into_iter()
            .filter(|i| i.kind == Some(kind))
            .map(|i| i.label)
            .collect(),
        _ => Vec::new(),
    }
}

/// Symbols offered with the cursor at the end of `text`, appended to `DEFS`.
fn symbol_completions(settings: Settings, text: &str) -> Vec<String> {
    completions(settings, text, "", CompletionItemKind::CONSTANT)
}

/// Keywords offered with the cursor at the end of `text`, appended to `DEFS`.
fn keyword_completions(text: &str) -> Vec<String> {
    completions(Settings::default(), text, "", CompletionItemKind::KEYWORD)
}

fn zephyr() -> Settings {
    Settings {
        zephyr_extensions: true,
        ..Default::default()
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
        "\tdepends on BAR != ",
        "\tdepends on BAR < ",
        "\tdepends on BAR >= ",
        "\tdepends on FOO || ",
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
        "\tdepends on (FOO) ",
        "\tdefault $(",
        "$(warning x) if ",
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

#[test]
fn no_symbols_in_help_text() {
    for text in [
        "config BAZ\n\tbool \"baz\"\n\thelp\n\t  if ",
        "config BAZ\n\tbool \"baz\"\n\thelp\n\t  Some text.\n\n\t  select this only if ",
        "config BAZ\n\tbool \"baz\"\n\thelp\n\t  Some text.\n\t  on some boards, if ",
        "config BAZ\n\tbool \"baz\"\n\thelp\n\t  Some text.\n\t  ( ",
        "menu \"m\"\nif FOO\nconfig BAZ\n\tbool \"baz\"\n\thelp\n\t  default ",
    ] {
        assert!(
            symbol_completions(Settings::default(), text).is_empty(),
            "text {text:?}"
        );
    }
}

#[test]
fn keywords_offered_where_a_line_starts() {
    for text in [
        "",
        "\t",
        "  ",
        "config BAZ\n\tbool \"baz\"\n\t",
        "config BAZ\n\tbool \"baz\"\n\thelp\n\t  Some text.\n\t",
    ] {
        let keywords = keyword_completions(text);
        assert!(keywords.iter().any(|k| k == "config"), "text {text:?}");
        assert!(keywords.iter().any(|k| k == "depends"), "text {text:?}");
    }
}

#[test]
fn keywords_offered_before_help() {
    let keywords = completions(
        Settings::default(),
        "config BAZ\n\tbool \"baz\"\n\t",
        "help\n\t  Text.\n",
        CompletionItemKind::KEYWORD,
    );
    assert!(keywords.iter().any(|k| k == "depends"));
}

#[test]
fn no_keywords_without_prefix_after_text() {
    for text in [
        "config ",
        "\tbool ",
        "\tbool \"use if ",
        "\tdepends on ",
        "\tdepends on FOO && ",
        "\tdefault y if ",
        "# ",
        "config BAZ\n\tbool \"baz\"\n\thelp\n\t  Say Y here if ",
    ] {
        assert!(keyword_completions(text).is_empty(), "text {text:?}");
    }
}

#[test]
fn no_keywords_on_a_blank_line_in_help_text() {
    let keywords = completions(
        Settings::default(),
        "config BAZ\n\tbool \"baz\"\n\thelp\n\t  Para one.\n\t  ",
        "\n\t  Para two.\n",
        CompletionItemKind::KEYWORD,
    );
    assert!(keywords.is_empty());
}

#[test]
fn keyword_prefix_still_matches_anywhere() {
    assert_eq!(
        keyword_completions("\tbool \"a\" i"),
        ["if", "int", "imply"]
    );
}

#[test]
fn symbols_offered_after_help_text() {
    let text = "config BAZ\n\tbool \"baz\"\n\thelp\n\t  Some text.\n\tdepends on ";
    let mut syms = symbol_completions(Settings::default(), text);
    syms.sort();
    assert_eq!(syms, ["BAR", "BAZ", "FOO"]);
}

#[test]
fn config_without_a_name_is_no_symbol() {
    let text = "config\n\nconfig BAZ\n\tbool \"baz\"\n\tdepends on ";
    let mut syms = symbol_completions(Settings::default(), text);
    syms.sort();
    assert_eq!(syms, ["BAR", "BAZ", "FOO"]);
}

#[test]
fn symbol_without_definitions_is_not_offered() {
    let mut index = WorldIndex::new();
    let other = Path::new("test/Kconfig.other");
    index.analyze_file(other, "config OLD\n\tbool \"old\"\n");
    let path = Path::new("test/Kconfig");
    index.analyze_file(path, "config A\n\tbool \"a\"\n\tdepends on ");
    // A change to the other file takes away the last definition of `OLD`.
    index.reanalyze_file(other, "config NEW\n\tbool \"new\"\n");
    let mut labels: Vec<String> = match completion::complete(&index, path, Position::new(2, 12)) {
        Some(CompletionResponse::Array(items)) => items.into_iter().map(|i| i.label).collect(),
        _ => Vec::new(),
    };
    labels.sort();
    assert_eq!(labels, ["A", "NEW"]);
}
