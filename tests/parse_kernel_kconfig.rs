use kconfig_lsp::analysis::WorldIndex;
use kconfig_lsp::ast::*;
use kconfig_lsp::lexer::{Lexer, TokenKind};
use kconfig_lsp::parser;
use kconfig_lsp::settings::Settings;
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
    let tokens = Lexer::new(SAMPLE_KCONFIG, &Settings::default()).tokenize();
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
    assert!(kinds.contains(&&DefType(TypeKind::Bool)));
    assert!(kinds.contains(&&DefType(TypeKind::Tristate)));
    assert!(kinds.contains(&&Range));
}

#[test]
fn parser_produces_correct_entries() {
    let tokens = Lexer::new(SAMPLE_KCONFIG, &Settings::default()).tokenize();
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

    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == DiagSeverity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected parse errors: {:?}", errors);
}

#[test]
fn analysis_finds_all_symbols() {
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

/// Run with `KCONFIG_LINUX_DIR=/path/to/linux cargo test -- --ignored`.
#[test]
#[ignore = "needs a Linux source tree in KCONFIG_LINUX_DIR"]
fn parse_real_kernel_kconfig() {
    let dir = std::env::var_os("KCONFIG_LINUX_DIR")
        .expect("KCONFIG_LINUX_DIR should point to a Linux source tree");
    let path = Path::new(&dir).join("init/Kconfig");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let tokens = Lexer::new(&source, &Settings::default()).tokenize();
    let result = parser::parse(&source, tokens);
    assert!(result.file.entries.len() > 10);

    let mut index = WorldIndex::new();
    index.analyze_file(&path, &source);
    assert!(index.all_symbols.len() > 20);
}

#[test]
fn help_text_does_not_swallow_next_entry() {
    let src = "config AUDIT\n\tbool \"Auditing support\"\n\tdepends on NET\n\tdefault y\n\thelp\n\t  Enable auditing infrastructure that can be used with another\n\t  kernel subsystem, such as SELinux.\n\nmenuconfig MODULES\n\tbool \"Enable loadable module support\"\n\tmodules\n";
    let tokens = Lexer::new(src, &Settings::default()).tokenize();
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

    assert!(
        names.contains(&"AUDIT".to_string()),
        "AUDIT missing from {:?}",
        names
    );
    assert!(
        names.contains(&"MODULES".to_string()),
        "MODULES missing from {:?}",
        names
    );
}

fn help_attr(file: &KconfigFile, name: &str) -> HelpAttr {
    file.entries
        .iter()
        .find_map(|e| match e {
            Entry::Config(c) if c.name == name => c.attributes.iter().find_map(|a| match a {
                Attribute::Help(h) => Some(h.clone()),
                _ => None,
            }),
            _ => None,
        })
        .expect("help attribute")
}

#[test]
fn help_span_covers_the_whole_text() {
    let src =
        "config A\n\tbool \"a\"\n\thelp\n\t  Line one.\n\t  Line two.\n\nconfig B\n\tbool \"b\"\n";
    let tokens = Lexer::new(src, &Settings::default()).tokenize();
    let result = parser::parse(src, tokens);
    let span = help_attr(&result.file, "A").span;
    assert_eq!(
        &src[span.start..span.end],
        "help\n\t  Line one.\n\t  Line two."
    );
}

#[test]
fn help_without_text_at_end_of_file() {
    let src = "config A\n\tbool \"a\"\n\thelp";
    let tokens = Lexer::new(src, &Settings::default()).tokenize();
    let result = parser::parse(src, tokens);
    assert_eq!(help_attr(&result.file, "A").text, "");
}

fn config_names(file: &KconfigFile) -> Vec<&str> {
    file.entries
        .iter()
        .filter_map(|e| match e {
            Entry::Config(c) => Some(c.name.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn help_skips_leading_blank_lines() {
    let src = "config A\n\tbool \"a\"\n\thelp\n\n\t  Text.\n";
    let tokens = Lexer::new(src, &Settings::default()).tokenize();
    let result = parser::parse(src, tokens);
    assert_eq!(help_attr(&result.file, "A").text, "Text.");
}

#[test]
fn help_ends_at_an_unindented_line() {
    for src in [
        "config A\n\tbool \"a\"\n\thelp\nconfig B\n\tbool \"b\"\n",
        "config A\n\tbool \"a\"\n\thelp\n\nconfig B\n\tbool \"b\"\n",
    ] {
        let tokens = Lexer::new(src, &Settings::default()).tokenize();
        let result = parser::parse(src, tokens);
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(help_attr(&result.file, "A").text, "", "{src:?}");
        assert_eq!(config_names(&result.file), ["A", "B"], "{src:?}");
    }
}

#[test]
fn help_indent_counts_tabs_as_eight_columns() {
    // `\t  ` is 10 columns, so 12 spaces are 2 more and 4 spaces end the text.
    let src =
        "config A\n\tbool \"a\"\n\thelp\n\t  Line one.\n            Line two.\n    depends on B\n";
    let tokens = Lexer::new(src, &Settings::default()).tokenize();
    let result = parser::parse(src, tokens);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(help_attr(&result.file, "A").text, "Line one.\n  Line two.");
    let has_depends_on = result.file.entries.iter().any(|e| match e {
        Entry::Config(c) => c
            .attributes
            .iter()
            .any(|a| matches!(a, Attribute::DependsOn(_))),
        _ => false,
    });
    assert!(has_depends_on);

    // A tab after spaces goes to the next tab stop, so `  \t  ` is 10 columns too.
    let src = "config A\n\tbool \"a\"\n\thelp\n          Line one.\n  \t  Line two.\n";
    let tokens = Lexer::new(src, &Settings::default()).tokenize();
    let result = parser::parse(src, tokens);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(help_attr(&result.file, "A").text, "Line one.\nLine two.");
}

#[test]
fn help_text_with_crlf_line_endings() {
    let body: String = (0..12)
        .map(|i| format!("\t  Line {i} depends on foo.\r\n"))
        .collect();
    let src =
        format!("config A\r\n\tbool \"a\"\r\n\thelp\r\n{body}\r\nconfig B\r\n\tbool \"b\"\r\n");
    let tokens = Lexer::new(&src, &Settings::default()).tokenize();
    let result = parser::parse(&src, tokens);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let help = help_attr(&result.file, "A");
    assert!(help.text.starts_with("Line 0 depends on foo.\nLine 1"));
    assert!(src[..help.span.end].ends_with("Line 11 depends on foo."));
    assert!(
        result
            .file
            .entries
            .iter()
            .any(|e| matches!(e, Entry::Config(c) if c.name == "B"))
    );
}

#[test]
fn unclosed_macro_ends_at_the_end_of_the_line() {
    for (line, mac) in [
        ("\tdefault $(foo", "$(foo"),
        ("\tprompt $(foo,(bar)", "$(foo,(bar)"),
    ] {
        let src = format!("config A\n{line}\nconfig B\n\tbool \"b\"\n");
        let tokens = Lexer::new(&src, &Settings::default()).tokenize();
        let result = parser::parse(&src, tokens);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.severity == DiagSeverity::Error)
            .map(|d| (d.message.as_str(), &src[d.span.start..d.span.end]))
            .collect();
        assert_eq!(errors, [("expected `)`", mac)], "{src:?}");
        assert_eq!(config_names(&result.file), ["A", "B"], "{src:?}");
    }

    // Help text is not Kconfig, so a macro in it is not checked.
    let src = "config A\n\tbool \"a\"\n\thelp\n\t  See $(srctree.\nconfig B\n\tbool \"b\"\n";
    let tokens = Lexer::new(src, &Settings::default()).tokenize();
    let result = parser::parse(src, tokens);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(help_attr(&result.file, "A").text, "See $(srctree.");
    assert_eq!(config_names(&result.file), ["A", "B"]);
}

#[test]
fn depends_on_with_if_condition() {
    let src = "config A\n\tbool \"a\"\n\tdepends on (B && C) if D\n";
    let tokens = Lexer::new(src, &Settings::default()).tokenize();
    let result = parser::parse(src, tokens);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let Some(Entry::Config(a)) = result.file.entries.first() else {
        panic!("expected config A");
    };
    let span = a
        .attributes
        .iter()
        .find_map(|attr| match attr {
            Attribute::DependsOn(d) => Some(d.span),
            _ => None,
        })
        .unwrap();
    assert_eq!(&src[span.start..span.end], "depends on (B && C) if D");

    let mut index = WorldIndex::new();
    index.analyze_file(Path::new("test/Kconfig"), src);
    for name in ["B", "C", "D"] {
        assert_eq!(index.get_references(name).len(), 1, "{name}");
    }
}

#[test]
fn depends_needs_on_and_visible_needs_if() {
    for (src, keyword, message) in [
        (
            "config A\n\tbool \"a\"\n\tdepends B\n",
            "depends",
            "expected `on` after `depends`",
        ),
        (
            "menu \"m\"\n\tvisible B\nendmenu\n",
            "visible",
            "expected `if` after `visible`",
        ),
    ] {
        let tokens = Lexer::new(src, &Settings::default()).tokenize();
        let result = parser::parse(src, tokens);
        let diags: Vec<_> = result
            .diagnostics
            .iter()
            .map(|d| {
                (
                    &src[d.span.start..d.span.end],
                    d.message.as_str(),
                    d.severity,
                )
            })
            .collect();
        assert_eq!(diags, [(keyword, message, DiagSeverity::Error)], "{src:?}");

        // The expression is still read, so `B` stays a reference.
        let mut index = WorldIndex::new();
        index.analyze_file(Path::new("test/Kconfig"), src);
        assert_eq!(index.get_references("B").len(), 1, "{src:?}");
    }
}

#[test]
fn attribute_span_ends_after_a_closing_paren() {
    for (src, want) in [
        (
            "config A\n\tbool \"a\"\n\tdepends on B if (C)\n",
            "depends on B if (C)",
        ),
        (
            "config A\n\tbool \"a\"\n\tdepends on B if (C) # x\n",
            "depends on B if (C)",
        ),
        (
            "config A\n\tbool \"a\"\n\tdepends on B if (C)",
            "depends on B if (C)",
        ),
        (
            "config A\n\tbool \"a\"\n\tdepends on (B || C)\n",
            "depends on (B || C)",
        ),
        (
            "config A\n\tbool \"a\"\n\tdepends on !(B)\n",
            "depends on !(B)",
        ),
    ] {
        let tokens = Lexer::new(src, &Settings::default()).tokenize();
        let result = parser::parse(src, tokens);
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        let Some(Entry::Config(a)) = result.file.entries.first() else {
            panic!("expected config A");
        };
        let span = a
            .attributes
            .iter()
            .find_map(|attr| match attr {
                Attribute::DependsOn(d) => Some(d.span),
                _ => None,
            })
            .unwrap();
        assert_eq!(&src[span.start..span.end], want, "{src:?}");
    }
}

#[test]
fn macro_variables_and_calls_make_no_entries() {
    let src = r#"comma := ,
quote := "
left_paren := (
empty :=
cc-info := $(shell,$(CC) --version) # not a comment
if-success = $(shell,{ $(1); } >/dev/null 2>&1 && echo "$(2)" || echo "$(3)")
flags += -O2
$(X)$(Y) := 5
CONFIG_BPF=y
$(error-if,$(failure,command -v $(CC)),C compiler '$(CC)' not found)
$(info,a) $(info,b)

menu "m"
inner := x
endmenu

if A
inner := y
endif

config B
	bool "b"
	depends on C = y
config_c := z
config C
	bool "c"
"#;
    let tokens = Lexer::new(src, &Settings::default()).tokenize();
    let result = parser::parse(src, tokens);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(config_names(&result.file), ["B", "C"]);
    assert_eq!(result.file.entries.len(), 4);
}

#[test]
fn macro_variable_value_is_the_rest_of_the_line() {
    for (src, op, value) in [
        ("x := a # b\n", ":=", Some("a # b")),
        ("quote := \"\n", ":=", Some("\"")),
        ("flags += -O2\n", "+=", Some("-O2")),
        ("CONFIG_BPF=y\r\n", "=", Some("y")),
        ("x := a \\\n", ":=", Some("a \\")),
        ("empty :=\n", ":=", None),
    ] {
        let tokens = Lexer::new(src, &Settings::default()).tokenize();
        let assign = tokens.iter().find(|t| t.kind == TokenKind::Assign);
        assert_eq!(assign.map(|t| &src[t.span.start..t.span.end]), Some(op));
        let got = tokens.iter().find_map(|t| match &t.kind {
            TokenKind::AssignValue(v) => Some(v.as_str()),
            _ => None,
        });
        assert_eq!(got, value, "{src:?}");
    }
}

#[test]
fn macro_variable_name_is_one_word() {
    for src in [
        "defaul FOO = y\n",
        "foo bar := x\n",
        "foo\n",
        "$(info,a) foo\n",
    ] {
        let tokens = Lexer::new(src, &Settings::default()).tokenize();
        let result = parser::parse(src, tokens);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.severity == DiagSeverity::Error),
            "{src:?}"
        );
    }
}

#[test]
fn macro_call_lines_do_not_end_attributes() {
    let src = "config A\n\tbool \"a\"\n$(info,x)\n\tdefault y\n\t$(warning,w) # c\n\thelp\n\t  Text.\n\nchoice\n\tprompt \"c\"\n\t$(info,y)\n\tdefault B\n\nconfig B\n\tbool \"b\"\n\nendchoice\n";
    let tokens = Lexer::new(src, &Settings::default()).tokenize();
    let result = parser::parse(src, tokens);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let [Entry::Config(a), Entry::Choice(c)] = result.file.entries.as_slice() else {
        panic!("expected config A and a choice: {:?}", result.file.entries);
    };
    assert!(
        a.attributes
            .iter()
            .any(|x| matches!(x, Attribute::Default(_)))
    );
    assert_eq!(help_attr(&result.file, "A").text, "Text.");
    assert!(
        c.attributes
            .iter()
            .any(|x| matches!(x, Attribute::Default(_)))
    );
}

#[test]
fn macro_call_line_errors() {
    for (line, want) in [
        ("$(info,x))", "unexpected token at top level"),
        ("$(warning,broken", "expected `)`"),
    ] {
        let src = format!("{line}\nconfig B\n\tbool \"b\"\n");
        let tokens = Lexer::new(&src, &Settings::default()).tokenize();
        let result = parser::parse(&src, tokens);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.severity == DiagSeverity::Error)
            .map(|d| d.message.as_str())
            .collect();
        assert_eq!(errors, [want], "{src:?}");
        assert_eq!(config_names(&result.file), ["B"], "{src:?}");
    }
}

#[test]
fn macro_lines_at_end_of_file() {
    for src in ["x := 1", "$(info,x)", "config A\n\tbool \"a\"\n\t$(info,x)"] {
        let tokens = Lexer::new(src, &Settings::default()).tokenize();
        let result = parser::parse(src, tokens);
        assert!(
            result.diagnostics.is_empty(),
            "{src:?}: {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn assignment_in_help_text_is_text() {
    let src =
        "config A\n\tbool \"a\"\n\thelp\n\t  CONFIG_FOO=y and x := $(y\nconfig B\n\tbool \"b\"\n";
    let tokens = Lexer::new(src, &Settings::default()).tokenize();
    let result = parser::parse(src, tokens);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(
        help_attr(&result.file, "A").text,
        "CONFIG_FOO=y and x := $(y"
    );
    assert_eq!(config_names(&result.file), ["A", "B"]);
}

#[test]
fn strings_keep_non_ascii_characters() {
    for (src, want) in [
        ("\"OLPC CAFÉ NAND\"", "OLPC CAFÉ NAND"),
        ("'naïve \\é'", "naïve é"),
    ] {
        let tokens = Lexer::new(src, &Settings::default()).tokenize();
        assert_eq!(
            tokens[0].kind,
            TokenKind::StringLit(want.to_string()),
            "{src:?}"
        );
    }
}
