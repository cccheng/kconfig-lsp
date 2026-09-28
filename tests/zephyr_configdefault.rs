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
