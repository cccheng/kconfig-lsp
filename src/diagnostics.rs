use std::path::Path;

use tower_lsp::lsp_types::{self as lsp, DiagnosticSeverity};

use crate::analysis::{FileAnalysis, WorldIndex};
use crate::ast::{DiagSeverity, Span};
use crate::checks;

pub fn collect(index: &WorldIndex, path: &Path) -> Vec<lsp::Diagnostic> {
    let fa = match index.files.get(path) {
        Some(fa) => fa,
        None => return Vec::new(),
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
            if r.file != path {
                continue;
            }
            if index.get_definitions(&r.name).is_empty()
                && !is_well_known_symbol(&r.name)
                && !r.name.starts_with("$(")
            {
                diags.push(diagnostic(
                    fa,
                    r.span,
                    DiagnosticSeverity::WARNING,
                    format!("symbol `{}` is not defined in any open file", r.name),
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

fn is_well_known_symbol(name: &str) -> bool {
    matches!(
        name,
        "y" | "n"
            | "m"
            | "MODULES"
            | "COMPILE_TEST"
            | "EXPERT"
            | "NET"
            | "BLOCK"
            | "SMP"
            | "PCI"
            | "USB"
            | "HAS_IOMEM"
            | "HAS_DMA"
            | "MMU"
            | "OF"
            | "ACPI"
            | "PM"
            | "ARCH_HAS_DMA_PREP_COHERENT"
    )
}
