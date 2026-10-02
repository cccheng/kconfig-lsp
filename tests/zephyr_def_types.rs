use kconfig_lsp::analysis::WorldIndex;
use kconfig_lsp::ast::DiagSeverity;
use kconfig_lsp::lexer::{Lexer, TokenKind, TypeKind};
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

fn index_with(settings: Settings) -> WorldIndex {
    let mut index = WorldIndex::new();
    index.settings = settings;
    index.analyze_file(Path::new("test/Kconfig"), SAMPLE_KCONFIG);
    index
}

fn type_of(index: &WorldIndex, name: &str) -> Option<TypeKind> {
    let defs = index.get_definitions(name);
    assert_eq!(defs.len(), 1, "{name} should be defined once");
    defs[0].type_kind
}

#[test]
fn lexer_tokenizes_def_types() {
    let tokens = Lexer::new(SAMPLE_KCONFIG, &settings()).tokenize();
    let kinds: Vec<_> = tokens.iter().map(|t| &t.kind).collect();
    for kind in [TypeKind::Int, TypeKind::Hex, TypeKind::String] {
        assert!(kinds.contains(&&TokenKind::DefType(kind)), "{kind:?}");
    }
}

#[test]
fn def_types_set_the_symbol_type() {
    let index = index_with(settings());

    assert_eq!(type_of(&index, "TEST_USES_DEF_INT"), Some(TypeKind::Int));
    assert_eq!(type_of(&index, "TEST_USES_DEF_HEX"), Some(TypeKind::Hex));
    assert_eq!(
        type_of(&index, "TEST_USES_DEF_STRING"),
        Some(TypeKind::String)
    );

    let diagnostics = &index.files[Path::new("test/Kconfig")].diagnostics;
    assert!(
        !diagnostics
            .iter()
            .any(|d| d.severity == DiagSeverity::Error),
        "unexpected parse errors: {diagnostics:?}"
    );
}

// The extension is opt-in: without it these are not keywords, so the
// symbols get no type and the lines are reported as errors.
#[test]
fn def_types_are_not_keywords_when_extension_disabled() {
    let index = index_with(Settings::default());

    for name in [
        "TEST_USES_DEF_INT",
        "TEST_USES_DEF_HEX",
        "TEST_USES_DEF_STRING",
    ] {
        assert_eq!(type_of(&index, name), None, "{name}");
    }

    let diagnostics = &index.files[Path::new("test/Kconfig")].diagnostics;
    assert!(
        diagnostics
            .iter()
            .any(|d| d.severity == DiagSeverity::Error),
        "def_* should be rejected when zephyr_extensions is off"
    );
}
