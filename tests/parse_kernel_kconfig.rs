use kconfig_lsp::analysis::WorldIndex;
use kconfig_lsp::ast::*;
use kconfig_lsp::lexer::Lexer;
use kconfig_lsp::parser;
use std::path::Path;

const SAMPLE_KCONFIG: &str = r#"
mainmenu "Linux Kernel Configuration"

config AUDIT
	bool "Auditing support"
	depends on NET
	default y
	help
	  Enable auditing infrastructure that can be used with another
	  kernel subsystem, such as SELinux.

menuconfig MODULES
	bool "Enable loadable module support"
	modules
	help
	  Kernel modules are small pieces of compiled code which can
	  be inserted in the running kernel.

config MODVERSIONS
	bool "Module versioning support"
	depends on MODULES
	help
	  Usually, modules have to be recompiled whenever you switch
	  to a new kernel.

menu "General setup"
	depends on !UML

config SYSVIPC
	bool "System V IPC"
	help
	  Inter Process Communication is a suite of library functions.

choice
	prompt "Compiler optimization level"
	default CC_OPTIMIZE_FOR_PERFORMANCE

config CC_OPTIMIZE_FOR_PERFORMANCE
	bool "Optimize for performance (-O2)"

config CC_OPTIMIZE_FOR_SIZE
	bool "Optimize for size (-Os)"

endchoice

if EXPERT

config CHECKPOINT_RESTORE
	bool "Checkpoint/restore support"
	select PROC_CHILDREN
	default n

endif

source "kernel/Kconfig.hz"

config SYSCTL
	bool "Sysctl support" if EXPERT
	depends on PROC_FS
	select PROC_SYSCTL
	imply SYSCTL_EXCEPTION_TRACE
	default y
	help
	  The sysctl interface.

config FOO_RANGE
	int "Foo value"
	range 1 100
	default 50

config HAS_FEATURE
	def_bool y

config OPTIONAL_FEATURE
	def_tristate m if MODULES

config NEW_OPT
	bool "New option"
	default OLD_OPT

config OLD_OPT
	bool
	transitional

endmenu
"#;

#[test]
fn lexer_tokenizes_all_keywords() {
    let tokens = Lexer::new(SAMPLE_KCONFIG).tokenize();
    assert!(tokens.len() > 50);

    let kinds: Vec<_> = tokens.iter().map(|t| &t.kind).collect();
    use kconfig_lsp::lexer::TokenKind::*;
    assert!(kinds.contains(&&Config));
    assert!(kinds.contains(&&MenuConfig));
    assert!(kinds.contains(&&Menu));
    assert!(kinds.contains(&&EndMenu));
    assert!(kinds.contains(&&Choice));
    assert!(kinds.contains(&&EndChoice));
    assert!(kinds.contains(&&If));
    assert!(kinds.contains(&&EndIf));
    assert!(kinds.contains(&&Source));
    assert!(kinds.contains(&&MainMenu));
    assert!(kinds.contains(&&Bool));
    assert!(kinds.contains(&&Int));
    assert!(kinds.contains(&&Default));
    assert!(kinds.contains(&&Depends));
    assert!(kinds.contains(&&On));
    assert!(kinds.contains(&&Select));
    assert!(kinds.contains(&&Imply));
    assert!(kinds.contains(&&Help));
    assert!(kinds.contains(&&Modules));
    assert!(kinds.contains(&&Transitional));
    assert!(kinds.contains(&&DefBool));
    assert!(kinds.contains(&&DefTristate));
    assert!(kinds.contains(&&Range));
}

#[test]
fn parser_produces_correct_entries() {
    let tokens = Lexer::new(SAMPLE_KCONFIG).tokenize();
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

    assert!(names.contains(&"AUDIT".to_string()));
    assert!(names.contains(&"MODULES".to_string()));

    let has_menu = result
        .file
        .entries
        .iter()
        .any(|e| matches!(e, Entry::Menu(_)));
    assert!(has_menu);

    let has_mainmenu = result
        .file
        .entries
        .iter()
        .any(|e| matches!(e, Entry::MainMenu(_)));
    assert!(has_mainmenu);

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
    let tokens = Lexer::new(SAMPLE_KCONFIG).tokenize();
    let result = parser::parse(SAMPLE_KCONFIG, tokens);
    let _ = result;

    let mut index = WorldIndex::new();
    index.analyze_file(Path::new("test/Kconfig"), SAMPLE_KCONFIG);

    let expected = [
        "AUDIT",
        "MODULES",
        "MODVERSIONS",
        "SYSVIPC",
        "CC_OPTIMIZE_FOR_PERFORMANCE",
        "CC_OPTIMIZE_FOR_SIZE",
        "CHECKPOINT_RESTORE",
        "SYSCTL",
        "FOO_RANGE",
        "HAS_FEATURE",
        "OPTIONAL_FEATURE",
        "NEW_OPT",
        "OLD_OPT",
    ];
    for sym in &expected {
        assert!(
            !index.get_definitions(sym).is_empty(),
            "symbol {} should be defined",
            sym
        );
    }

    let audit_defs = index.get_definitions("AUDIT");
    assert_eq!(audit_defs[0].type_kind, Some(TypeKind::Bool));
    assert_eq!(audit_defs[0].prompt.as_deref(), Some("Auditing support"));
    assert!(audit_defs[0].help.is_some());

    let modules_defs = index.get_definitions("MODULES");
    assert_eq!(
        modules_defs[0].kind,
        kconfig_lsp::analysis::DefKind::MenuConfig
    );

    let old_opt_defs = index.get_definitions("OLD_OPT");
    assert_eq!(old_opt_defs[0].type_kind, Some(TypeKind::Bool));

    let has_feature_defs = index.get_definitions("HAS_FEATURE");
    assert_eq!(has_feature_defs[0].type_kind, Some(TypeKind::Bool));

    let net_refs = index.get_references("NET");
    assert!(!net_refs.is_empty(), "NET should be referenced");

    let proc_children_refs = index.get_references("PROC_CHILDREN");
    assert!(
        !proc_children_refs.is_empty(),
        "PROC_CHILDREN should be referenced via select"
    );
}

#[test]
fn help_text_parsed_correctly() {
    let mut index = WorldIndex::new();
    index.analyze_file(Path::new("test/Kconfig"), SAMPLE_KCONFIG);

    let audit_help = index.get_definitions("AUDIT")[0].help.as_ref().unwrap();
    assert!(audit_help.starts_with("Enable auditing"));
    assert!(audit_help.contains("SELinux"));
    assert!(!audit_help.starts_with("\t"));
    assert!(!audit_help.starts_with("  "));
}

#[test]
fn parse_real_kernel_kconfig() {
    let path = Path::new("/home/cccheng/Workspace/linux/init/Kconfig");
    if !path.exists() {
        eprintln!("skipping: init/Kconfig not found");
        return;
    }
    let source = std::fs::read_to_string(path).unwrap();
    let tokens = Lexer::new(&source).tokenize();
    let result = parser::parse(&source, tokens);

    assert!(result.file.entries.len() > 10);
    eprintln!(
        "init/Kconfig: {} entries, {} diagnostics",
        result.file.entries.len(),
        result.diagnostics.len()
    );

    let mut index = WorldIndex::new();
    index.analyze_file(path, &source);
    eprintln!("Symbols: {}", index.all_symbols.len());
    assert!(index.all_symbols.len() > 20);
}

#[test]
fn debug_help_consumption() {
    let src = "config AUDIT\n\tbool \"Auditing support\"\n\tdepends on NET\n\tdefault y\n\thelp\n\t  Enable auditing infrastructure that can be used with another\n\t  kernel subsystem, such as SELinux.\n\nmenuconfig MODULES\n\tbool \"Enable loadable module support\"\n\tmodules\n";
    let tokens = Lexer::new(src).tokenize();
    let result = parser::parse(src, tokens);

    let names: Vec<String> = result
        .file
        .entries
        .iter()
        .filter_map(|e| match e {
            Entry::Config(c) | Entry::MenuConfig(c) => Some(c.name.clone()),
            _ => None,
        })
        .collect();

    eprintln!("names: {:?}", names);
    for d in &result.diagnostics {
        eprintln!("  diag: {:?} {}", d.severity, d.message);
    }

    assert!(names.contains(&"AUDIT".to_string()), "AUDIT missing");
    assert!(
        names.contains(&"MODULES".to_string()),
        "MODULES missing from {:?}",
        names
    );
}
