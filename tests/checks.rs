use std::path::Path;

use kconfig_lsp::analysis::WorldIndex;
use kconfig_lsp::diagnostics;
use tower_lsp::lsp_types::DiagnosticSeverity;

const ERROR: DiagnosticSeverity = DiagnosticSeverity::ERROR;

/// Diagnostics of the first file as `(text, message, severity)`. The other
/// files are indexed too.
fn diags(files: &[(&str, &str)]) -> Vec<(String, String, DiagnosticSeverity)> {
    let mut index = WorldIndex::new();
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
