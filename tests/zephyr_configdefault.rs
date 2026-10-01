use kconfig_lsp::analysis::{RefKind, WorldIndex};
use kconfig_lsp::ast::*;
use kconfig_lsp::lexer::{Lexer, TokenKind, TypeKind};
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

// Every attribute keyword that `config` accepts, except `default`, must be
// rejected inside `configdefault` with a precise diagnostic (not fall through as
// an opaque top-level error). Enumerated explicitly so a keyword that stops being
// rejected here shows up as a failure.
#[test]
fn configdefault_rejects_every_non_default_attribute() {
    let keywords = [
        "bool",
        "tristate",
        "string",
        "hex",
        "int",
        "prompt",
        "def_bool",
        "def_tristate",
        "depends",
        "select",
        "imply",
        "visible",
        "range",
        "help",
        "modules",
        "transitional",
        "optional",
    ];

    for kw in keywords {
        let src = format!("configdefault FOO\n\t{kw}\n");
        let tokens = Lexer::new(&src, &settings()).tokenize();
        let result = parser::parse(&src, tokens);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.severity == DiagSeverity::Error
                    && d.message.contains("configdefault can only contain")),
            "[{kw}] expected a 'configdefault can only contain default' diagnostic, got: {:?}",
            result
                .diagnostics
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
    }
}

// Zephyr docs allow wrapping configdefault in an `if` block:
//   if BAR / configdefault FOO / default y / endif
#[test]
fn configdefault_inside_if_block() {
    let src = "\
if BAR
configdefault FOO
    default y
endif

config AFTER
    bool \"after\"
";
    let tokens = Lexer::new(src, &settings()).tokenize();
    let result = parser::parse(src, tokens);
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == DiagSeverity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // ConfigDefault(FOO) is nested inside the If(BAR) block.
    let if_entry = result
        .file
        .entries
        .iter()
        .find_map(|e| match e {
            Entry::If(i) => Some(i),
            _ => None,
        })
        .expect("if block should be parsed");
    assert!(
        if_entry
            .entries
            .iter()
            .any(|e| matches!(e, Entry::ConfigDefault(c) if c.name == "FOO")),
        "configdefault FOO should be nested inside the if block"
    );
    assert_eq!(
        sym(&if_entry.condition),
        "BAR",
        "the if condition should be BAR"
    );

    // Analysis: the entry after `endif` still parses, FOO is a reference
    // (configdefault augments a symbol) rather than a new definition, and the
    // outer `if` condition BAR is recorded as a condition reference.
    let mut index = WorldIndex::new();
    index.settings = settings();
    index.analyze_file(Path::new("Kconfig"), src);
    assert!(
        index
            .get_references("BAR")
            .iter()
            .any(|r| r.kind == RefKind::IfCondition),
        "BAR should be recorded as an if-condition reference"
    );
    assert!(
        !index.get_definitions("AFTER").is_empty(),
        "config AFTER after endif should be parsed"
    );
    assert!(
        index.get_definitions("FOO").is_empty(),
        "configdefault must not define FOO"
    );
    assert!(
        !index.get_references("FOO").is_empty(),
        "configdefault should reference FOO"
    );
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
    assert_eq!(
        defaults[0].condition.as_ref().map(sym),
        Some("COND".to_string())
    );
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

// With zephyr_extensions disabled (the default), `configdefault` is not a
// keyword. This keeps the extension opt-in so plain Linux/other Kconfig files are
// unaffected. Guards against a regression that makes the keyword unconditional.
#[test]
fn configdefault_not_a_keyword_when_extension_disabled() {
    let src = "configdefault FOO\n\tdefault y\n";
    let tokens = Lexer::new(src, &Settings::default()).tokenize();

    assert!(
        !tokens.iter().any(|t| t.kind == TokenKind::ConfigDefault),
        "configdefault must not tokenize as a keyword when the extension is off"
    );
    assert!(
        tokens
            .iter()
            .any(|t| matches!(&t.kind, TokenKind::Ident(s) if s == "configdefault")),
        "configdefault should be a plain identifier when the extension is off"
    );
}

// With the extension disabled a configdefault block is not valid syntax: it is
// rejected (error diagnostic) and yields no ConfigDefault entry.
#[test]
fn configdefault_rejected_when_extension_disabled() {
    let src = "configdefault FOO\n\tdefault y\n";
    let tokens = Lexer::new(src, &Settings::default()).tokenize();
    let result = parser::parse(src, tokens);

    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.severity == DiagSeverity::Error),
        "configdefault should be rejected when zephyr_extensions is off"
    );
    assert!(
        !result
            .file
            .entries
            .iter()
            .any(|e| matches!(e, Entry::ConfigDefault(_))),
        "no ConfigDefault entry should be produced when the extension is off"
    );
}
