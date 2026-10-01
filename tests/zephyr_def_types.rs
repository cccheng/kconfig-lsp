use kconfig_lsp::analysis::WorldIndex;
use kconfig_lsp::ast::DiagSeverity;
use kconfig_lsp::lexer::{Lexer, TypeKind};
use kconfig_lsp::parser;
use kconfig_lsp::settings::Settings;
use std::path::Path;

const SAMPLE_KCONFIG: &str = r#"
config TEST_USES_DEF_INT
    def_int 0

config TEST_USES_DEF_HEX
    def_hex 0xff

config TEST_USES_DEF_STRING
    def_string "foobar"
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
    assert!(kinds.contains(&&DefType(TypeKind::Int)));
    assert!(kinds.contains(&&DefType(TypeKind::Hex)));
    assert!(kinds.contains(&&DefType(TypeKind::String)));
}

#[test]
fn analysis_finds_all_symbols() {
    let tokens = Lexer::new(SAMPLE_KCONFIG, &settings()).tokenize();
    let result = parser::parse(SAMPLE_KCONFIG, tokens);

    let mut index = WorldIndex::new();
    index.settings = settings();
    index.analyze_file(Path::new("test/Kconfig"), SAMPLE_KCONFIG);

    let expected = [
        "TEST_USES_DEF_INT",
        "TEST_USES_DEF_HEX",
        "TEST_USES_DEF_STRING",
    ];
    for sym in &expected {
        assert!(
            !index.get_definitions(sym).is_empty(),
            "symbol {} should be defined",
            sym
        );
    }

    let test_defs = index.get_definitions("TEST_USES_DEF_INT");
    assert_eq!(test_defs[0].type_kind, Some(TypeKind::Int));
    let test_defs = index.get_definitions("TEST_USES_DEF_HEX");
    assert_eq!(test_defs[0].type_kind, Some(TypeKind::Hex));
    let test_defs = index.get_definitions("TEST_USES_DEF_STRING");
    assert_eq!(test_defs[0].type_kind, Some(TypeKind::String));

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
