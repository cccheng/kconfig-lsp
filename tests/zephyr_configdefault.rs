use kconfig_lsp::analysis::WorldIndex;
use kconfig_lsp::ast::*;
use kconfig_lsp::lexer::{Lexer, TypeKind};
use kconfig_lsp::parser;
use kconfig_lsp::settings::Settings;
use std::path::Path;

const SAMPLE_KCONFIG: &str = r#"
config TEST_CONFIG
	bool "test config"
	help
		foo

config TEST_CONDITION
    bool "test condition"
    default y

configdefault TEST_CONFIG
    default y if TEST_CONDITION
"#;

fn settings() -> Settings {
    Settings {
        zephyr_extensions: true,
    }
}

fn sym(e: &Expr) -> String {
    match e {
        Expr::Symbol(s, _) => s.clone(),
        other => panic!("expected a symbol expression, got {other:?}"),
    }
}

#[test]
fn lexer_tokenizes_all_keywords() {
    let tokens = Lexer::new(SAMPLE_KCONFIG, &settings()).tokenize();
    let kinds: Vec<_> = tokens.iter().map(|t| &t.kind).collect();
    use kconfig_lsp::lexer::TokenKind::*;
    assert!(kinds.contains(&&Config));
    assert!(kinds.contains(&&ConfigDefault));
    assert!(kinds.contains(&&Default));
}

#[test]
fn parser_produces_correct_entries() {
    let tokens = Lexer::new(SAMPLE_KCONFIG, &settings()).tokenize();
    let result = parser::parse(SAMPLE_KCONFIG, tokens);
    let names: Vec<String> = result
        .file
        .entries
        .iter()
        .filter_map(|e| match e {
            Entry::Config(c) | Entry::MenuConfig(c) => Some(c.name.clone()),
            _ => None,
        })
        .collect();
    assert!(names.contains(&"TEST_CONFIG".to_string()));
    assert!(names.contains(&"TEST_CONDITION".to_string()));

    for d in &result.diagnostics {
        eprintln!("  diag: {:?} {}", d.severity, d.message);
    }

    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == DiagSeverity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected parse errors: {:?}", errors);
}

#[test]
fn analysis_finds_all_symbols() {
    let tokens = Lexer::new(SAMPLE_KCONFIG, &settings()).tokenize();
    let result = parser::parse(SAMPLE_KCONFIG, tokens);

    let mut index = WorldIndex::new();
    index.settings = settings();
    index.analyze_file(Path::new("test/Kconfig"), SAMPLE_KCONFIG);

    let expected = ["TEST_CONFIG", "TEST_CONDITION"];
    for sym in &expected {
        assert!(
            !index.get_definitions(sym).is_empty(),
            "symbol {} should be defined",
            sym
        );
    }

    let audit_defs = index.get_definitions("TEST_CONFIG");
    assert_eq!(audit_defs[0].type_kind, Some(TypeKind::Bool));
    assert_eq!(audit_defs[0].prompt.as_deref(), Some("test config"));
    assert!(audit_defs[0].help.is_some());

    for d in &result.diagnostics {
        eprintln!("  diag: {:?} {}", d.severity, d.message);
    }

    let test_refs = index.get_references("TEST_CONDITION");
    assert!(
        !test_refs.is_empty(),
        "TEST_CONDITION should be referenced by configdefault"
    );

    let test_refs = index.get_references("TEST_CONFIG");
    assert!(
        !test_refs.is_empty(),
        "TEST_CONFIG should be referenced by configdefault"
    );
}

// An illegal attribute must be reported precisely, not fall through as an opaque
// top-level error.
#[test]
fn configdefault_rejects_non_default_attributes() {
    let cases = [
        ("depends on", "configdefault FOO\n\tdefault y\n\tdepends on BAR\n"),
        ("bool type", "configdefault FOO\n\tbool \"x\"\n\tdefault y\n"),
        ("select", "configdefault FOO\n\tselect BAR\n"),
    ];

    for (label, src) in cases {
        let tokens = Lexer::new(src, &settings()).tokenize();
        let result = parser::parse(src, tokens);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.severity == DiagSeverity::Error)
            .collect();

        assert!(
            errors
                .iter()
                .any(|d| d.message.contains("configdefault can only contain")),
            "[{label}] expected a 'configdefault can only contain default' diagnostic, got: {:?}",
            errors.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }
}

// `help` greedily consumes indented lines; an illegal one must not eat the entry
// that follows.
#[test]
fn configdefault_illegal_help_does_not_swallow_next_entry() {
    let src = "configdefault FOO\n\thelp\nconfig BAR\n\tbool \"bar\"\n";
    let tokens = Lexer::new(src, &settings()).tokenize();
    let result = parser::parse(src, tokens);

    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("configdefault can only contain")),
        "expected the illegal-help diagnostic"
    );

    let names: Vec<String> = result
        .file
        .entries
        .iter()
        .filter_map(|e| match e {
            Entry::Config(c) | Entry::MenuConfig(c) => Some(c.name.clone()),
            _ => None,
        })
        .collect();
    assert!(
        names.contains(&"BAR".to_string()),
        "config BAR after an illegal `help` should still be parsed, got {names:?}"
    );
}

// The lexer discards indentation, so an illegal `help` body that reads like
// `default X` must not be re-parsed as a default (and `X` must not be indexed).
#[test]
fn configdefault_illegal_help_body_is_not_reparsed_as_default() {
    let src = "configdefault FOO\n\thelp\n\t\tdefault MISSING\nconfig BAR\n\tbool \"bar\"\n";
    let mut index = WorldIndex::new();
    index.settings = settings();
    index.analyze_file(Path::new("Kconfig"), src);

    assert!(
        index.get_references("MISSING").is_empty(),
        "a `default MISSING` inside an illegal help body must not be indexed"
    );
    assert!(
        !index.get_definitions("BAR").is_empty(),
        "config BAR after the illegal help body should still be parsed"
    );
}

// A rejected attribute must not leak a reference into the index (which would
// raise a spurious undefined-symbol warning).
#[test]
fn configdefault_illegal_attr_contributes_no_reference() {
    let src = "config REAL\n\tbool \"r\"\n\nconfigdefault REAL\n\tdepends on MISSING\n";
    let mut index = WorldIndex::new();
    index.settings = settings();
    index.analyze_file(Path::new("Kconfig"), src);

    assert!(
        index.get_references("MISSING").is_empty(),
        "a rejected `depends on MISSING` should not create a reference"
    );
}

// A valid default-only configdefault parses cleanly and stops at the next entry.
#[test]
fn configdefault_accepts_default_only_and_stops_at_next_entry() {
    let src = "\
configdefault FOO
    default y if COND
    default n

config BAR
    bool \"bar\"
";
    let tokens = Lexer::new(src, &settings()).tokenize();
    let result = parser::parse(src, tokens);

    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == DiagSeverity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // FOO's configdefault must keep both defaults in the AST, with values and
    // conditions intact.
    let cd = result
        .file
        .entries
        .iter()
        .find_map(|e| match e {
            Entry::ConfigDefault(c) if c.name == "FOO" => Some(c),
            _ => None,
        })
        .expect("configdefault FOO should be parsed");
    let defaults: Vec<&DefaultAttr> = cd
        .attributes
        .iter()
        .map(|a| match a {
            Attribute::Default(d) => d,
            other => panic!("configdefault should only hold defaults, got {other:?}"),
        })
        .collect();
    assert_eq!(defaults.len(), 2, "expected two defaults");
    assert_eq!(sym(&defaults[0].value), "y");
    assert_eq!(defaults[0].condition.as_ref().map(sym), Some("COND".to_string()));
    assert_eq!(sym(&defaults[1].value), "n");
    assert!(defaults[1].condition.is_none());

    // The following `config BAR` must still be parsed as its own entry.
    let names: Vec<String> = result
        .file
        .entries
        .iter()
        .filter_map(|e| match e {
            Entry::Config(c) | Entry::MenuConfig(c) => Some(c.name.clone()),
            _ => None,
        })
        .collect();
    assert!(
        names.contains(&"BAR".to_string()),
        "config BAR after configdefault should be parsed as its own entry, got {names:?}"
    );
}
