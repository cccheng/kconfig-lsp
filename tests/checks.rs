use std::path::Path;

use kconfig_lsp::analysis::WorldIndex;
use kconfig_lsp::diagnostics;
use kconfig_lsp::settings::Settings;
use tower_lsp::lsp_types::DiagnosticSeverity;

const ERROR: DiagnosticSeverity = DiagnosticSeverity::ERROR;
const WARNING: DiagnosticSeverity = DiagnosticSeverity::WARNING;

fn diags(files: &[(&str, &str)]) -> Vec<(String, String, DiagnosticSeverity)> {
    diags_with(Settings::default(), files)
}

/// Diagnostics of the first file as `(text, message, severity)`. The other
/// files are indexed too.
fn diags_with(
    settings: Settings,
    files: &[(&str, &str)],
) -> Vec<(String, String, DiagnosticSeverity)> {
    let mut index = WorldIndex::new();
    index.settings = settings;
    for (path, src) in files {
        index.analyze_file(Path::new(path), src);
    }
    let path = Path::new(files[0].0);
    let fa = &index.files[path];
    let offset =
        |p: tower_lsp::lsp_types::Position| fa.line_index.offset(&fa.source, p.line, p.character);
    diagnostics::collect(&index, path)
        .into_iter()
        .map(|d| {
            let text = &fa.source[offset(d.range.start)..offset(d.range.end)];
            (text.to_string(), d.message, d.severity.unwrap())
        })
        .collect()
}

fn check(src: &str) -> Vec<(String, String, DiagnosticSeverity)> {
    diags(&[("Kconfig", src)])
}

fn zephyr() -> Settings {
    Settings {
        zephyr_extensions: true,
    }
}

fn diag(
    text: &str,
    message: &str,
    severity: DiagnosticSeverity,
) -> (String, String, DiagnosticSeverity) {
    (text.to_string(), message.to_string(), severity)
}

#[test]
fn help_given_twice_or_without_text() {
    let src = "\
config A
\tbool \"a\"
\thelp
\t  First.
\thelp
\t  Second.

choice
\tprompt \"c\"
\thelp
config C1
\tbool \"c1\"
\thelp
\t  Text.
endchoice
";
    assert_eq!(
        check(src),
        [
            diag("help", "more than one help text", ERROR),
            diag("help", "`help` without text", ERROR),
        ]
    );
}

#[test]
fn menuconfig_needs_a_prompt_and_symbols_need_a_type() {
    let src = "\
menuconfig A
\tbool
\tdepends on B

menuconfig B
\tbool \"b\"

menuconfig C
\ttristate
\tprompt \"c\"

config D
\tdepends on B
";
    assert_eq!(
        check(src),
        [
            diag("A", "menuconfig without a prompt", ERROR),
            diag("D", "no definition of `D` gives it a type", WARNING),
        ]
    );
    // The type can come from a definition in another file.
    let other = "config D\n\tbool \"d\"\n";
    assert_eq!(
        diags(&[("Kconfig", src), ("other/Kconfig", other)]),
        [diag("A", "menuconfig without a prompt", ERROR)]
    );
    // Zephyr can make the typed definition at build time.
    assert_eq!(
        diags_with(zephyr(), &[("Kconfig", src)]),
        [diag("A", "menuconfig without a prompt", ERROR)]
    );
    // A choice gives its type to a member without one.
    let choice = "\
choice
\tbool \"pick\"
config C1
\tprompt \"c1\"
endchoice
config E
\tdepends on C1
";
    assert_eq!(
        check(choice),
        [diag("E", "no definition of `E` gives it a type", WARNING)]
    );
}

#[test]
fn select_and_imply_work_only_with_bool_or_tristate() {
    let src = "\
config A
\tint \"a\"
\tselect B
\timply B

config B
\tbool

config C
\ttristate \"c\"
\tselect D
\timply F
\timply B

config D
\tint

config F
\thex
";
    let msg = |name: &str, t: &str, keyword: &str| {
        format!("`{name}` is {t}, but `{keyword}` works only with bool or tristate symbols")
    };
    assert_eq!(
        check(src),
        [
            diag("select B", &msg("A", "int", "select"), WARNING),
            diag("imply B", &msg("A", "int", "imply"), WARNING),
            diag("D", &msg("D", "int", "select"), WARNING),
            diag("F", &msg("F", "hex", "imply"), WARNING),
        ]
    );

    // The type can come from another file. Definitions that do not agree
    // on the type give no type.
    let src = "\
config A
\tdef_bool y
\tselect T
\tselect X

config X
\tint
";
    let other = "\
config T
\tstring

config X
\tbool
";
    assert_eq!(
        diags(&[("Kconfig", src), ("other/Kconfig", other)]),
        [diag("T", &msg("T", "string", "select"), WARNING)]
    );
}

#[test]
fn range_works_only_with_int_or_hex() {
    let src = "\
config A
\tbool \"a\"
\trange 1 5

config B
\tint \"b\"
\trange 1 5

config C
\thex \"c\"
\trange 0x10 0x20 if A

config S
\tstring \"s\"
\trange 0 10 if A
";
    let msg = |name: &str, t: &str| {
        format!("`{name}` is {t}, but `range` works only with int or hex symbols")
    };
    assert_eq!(
        check(src),
        [
            diag("range 1 5", &msg("A", "bool"), WARNING),
            diag("range 0 10 if A", &msg("S", "string"), WARNING),
        ]
    );
}

#[test]
fn int_and_hex_values_must_be_numbers() {
    let src = "\
config A
\tint \"a\"
\tdefault 010
\tdefault 0x10 if B
\tdefault y if B
\tdefault \"-12\" if B
\tdefault \"-1a\" if B
\tdefault ((\"bad\")) if B
\tdefault C if B
\tdefault H if B
\tdefault $(FOO) if B
\tdefault \"$(BAR)\" if B
\tdefault UNDEF if B
\trange 0 S

config B
\tbool \"b\"

config C
\tbool

config H
\thex \"h\"
\tdefault 10 if B
\tdefault \"0x\" if B
\trange 0X0 -0x10

config S
\tstring \"s\"

config X
\tdef_hex \"0xZZ\"

configdefault A
\tdefault \"z\" if B
";
    // Zephyr for `def_hex` and `configdefault`.
    assert_eq!(
        diags_with(zephyr(), &[("Kconfig", src)]),
        [
            diag("0x10", "`0x10` is not a valid int value", WARNING),
            diag("y", "`y` is not a valid int value", WARNING),
            diag("\"-1a\"", "`-1a` is not a valid int value", WARNING),
            diag("\"bad\"", "`bad` is not a valid int value", WARNING),
            diag("C", "`C` is bool, not int or hex", WARNING),
            diag("S", "`S` is string, not int or hex", WARNING),
            diag("\"0x\"", "`0x` is not a valid hex value", WARNING),
            diag("\"0xZZ\"", "`0xZZ` is not a valid hex value", WARNING),
            diag("\"z\"", "`z` is not a valid int value", WARNING),
            diag(
                "UNDEF",
                "symbol `UNDEF` is not defined in any open file",
                WARNING
            ),
        ]
    );
}
