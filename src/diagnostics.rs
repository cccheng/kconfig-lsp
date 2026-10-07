use std::path::Path;

use tower_lsp::lsp_types::{self as lsp, DiagnosticSeverity};

use crate::analysis::{FileAnalysis, WorldIndex};
use crate::ast::{DiagSeverity, Span};
use crate::checks;

pub fn collect(index: &WorldIndex, path: &Path) -> Vec<lsp::Diagnostic> {
    let Some((path, fa)) = index.files.get_key_value(path) else {
        return Vec::new();
    };

    let mut diags: Vec<lsp::Diagnostic> = Vec::new();

    for pd in fa.diagnostics.iter().chain(&checks::check(index, &fa.file)) {
        let severity = match pd.severity {
            DiagSeverity::Error => DiagnosticSeverity::ERROR,
            DiagSeverity::Warning => DiagnosticSeverity::WARNING,
        };
        diags.push(diagnostic(fa, pd.span, severity, pd.message.clone()));
    }

    for ref_entry in index.references.values() {
        for r in ref_entry {
            // The references of the file have the path of its key. Comparing
            // the bytes is much faster than comparing `Path` components.
            if r.file.as_os_str() != path.as_os_str() {
                continue;
            }
            if index.get_definitions(&r.name).is_empty()
                && !r.name.starts_with("$(")
                && !(index.settings.zephyr_extensions && is_made_by_zephyr(&r.name))
            {
                diags.push(diagnostic(
                    fa,
                    r.span,
                    DiagnosticSeverity::WARNING,
                    format!("symbol `{}` is not defined in the workspace", r.name),
                ));
            }
        }
    }

    diags
}

fn diagnostic(
    fa: &FileAnalysis,
    span: Span,
    severity: DiagnosticSeverity,
    message: String,
) -> lsp::Diagnostic {
    let (line, col) = fa.line_index.line_col(&fa.source, span.start);
    let (end_line, end_col) = fa.line_index.line_col(&fa.source, span.end);
    lsp::Diagnostic {
        range: lsp::Range {
            start: lsp::Position::new(line, col),
            end: lsp::Position::new(end_line, end_col),
        },
        severity: Some(severity),
        source: Some("kconfig-lsp".into()),
        message,
        ..Default::default()
    }
}

/// Zephyr makes most of these symbols at build time, from the devicetree
/// and the list of boards.
fn is_made_by_zephyr(name: &str) -> bool {
    (name.starts_with("DT_HAS_") && name.ends_with("_ENABLED")) || name.starts_with("BOARD_")
}
