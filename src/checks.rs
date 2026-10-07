use crate::ast::*;

/// Checks a file against the rules of the Linux Kconfig tools that the
/// parser does not apply.
pub fn check(file: &KconfigFile) -> Vec<ParseDiagnostic> {
    let mut out = Vec::new();
    walk(&file.entries, &mut |entry| match entry {
        Entry::Config(c) | Entry::MenuConfig(c) => check_help(&c.attributes, &mut out),
        Entry::Choice(c) => check_help(&c.attributes, &mut out),
        _ => {}
    });
    out
}

/// Calls `f` on each entry, also on those in menus, choices and `if`
/// blocks.
fn walk<'a>(entries: &'a [Entry], f: &mut impl FnMut(&'a Entry)) {
    for entry in entries {
        f(entry);
        match entry {
            Entry::Choice(c) => walk(&c.entries, f),
            Entry::Menu(m) => walk(&m.entries, f),
            Entry::If(i) => walk(&i.entries, f),
            _ => {}
        }
    }
}

fn check_help(attrs: &[Attribute], out: &mut Vec<ParseDiagnostic>) {
    let helps = attrs.iter().filter_map(|a| match a {
        Attribute::Help(h) => Some(h),
        _ => None,
    });
    for (i, h) in helps.enumerate() {
        let keyword = Span::new(h.span.start, h.span.start + "help".len());
        if i > 0 {
            push(out, keyword, "more than one help text", DiagSeverity::Error);
        }
        if h.text.is_empty() {
            push(out, keyword, "`help` without text", DiagSeverity::Error);
        }
    }
}

fn push(out: &mut Vec<ParseDiagnostic>, span: Span, message: &str, severity: DiagSeverity) {
    out.push(ParseDiagnostic {
        message: message.to_string(),
        span,
        severity,
    });
}
