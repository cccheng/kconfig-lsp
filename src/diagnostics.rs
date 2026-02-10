use std::path::Path;

use tower_lsp::lsp_types::{self as lsp, DiagnosticSeverity};

use crate::analysis::WorldIndex;
use crate::ast::DiagSeverity;

pub fn collect(index: &WorldIndex, path: &Path) -> Vec<lsp::Diagnostic> {
    let fa = match index.files.get(path) {
        Some(fa) => fa,
        None => return Vec::new(),
    };

    let mut diags: Vec<lsp::Diagnostic> = Vec::new();

    for pd in &fa.diagnostics {
        let (line, col) = fa.line_index.line_col(pd.span.start);
        let (end_line, end_col) = fa.line_index.line_col(pd.span.end);
        diags.push(lsp::Diagnostic {
            range: lsp::Range {
                start: lsp::Position::new(line, col),
                end: lsp::Position::new(end_line, end_col),
            },
            severity: Some(match pd.severity {
                DiagSeverity::Error => DiagnosticSeverity::ERROR,
                DiagSeverity::Warning => DiagnosticSeverity::WARNING,
            }),
            source: Some("kconfig-lsp".into()),
            message: pd.message.clone(),
            ..Default::default()
        });
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
                let (line, col) = fa.line_index.line_col(r.span.start);
                let (end_line, end_col) = fa.line_index.line_col(r.span.end);
                diags.push(lsp::Diagnostic {
                    range: lsp::Range {
                        start: lsp::Position::new(line, col),
                        end: lsp::Position::new(end_line, end_col),
                    },
                    severity: Some(DiagnosticSeverity::WARNING),
                    source: Some("kconfig-lsp".into()),
                    message: format!("symbol `{}` is not defined in any open file", r.name),
                    ..Default::default()
                });
            }
        }
    }

    diags
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
