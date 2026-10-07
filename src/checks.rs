use crate::analysis::WorldIndex;
use crate::ast::*;

/// Checks a file against the rules of the Linux Kconfig tools that the
/// parser does not apply. Some rules need the definitions in other files,
/// which `index` holds.
pub fn check(index: &WorldIndex, file: &KconfigFile) -> Vec<ParseDiagnostic> {
    let mut out = Vec::new();
    walk(&file.entries, false, &mut |entry, in_choice| match entry {
        Entry::Config(c) | Entry::MenuConfig(c) => {
            if matches!(entry, Entry::MenuConfig(_)) && !c.attributes.iter().any(is_prompt) {
                push(
                    &mut out,
                    c.name_span,
                    "menuconfig without a prompt",
                    DiagSeverity::Error,
                );
            }
            check_type(index, c, in_choice, &mut out);
            check_select(index, c, &mut out);
            check_range(index, c, &mut out);
            check_help(&c.attributes, &mut out);
        }
        Entry::Choice(c) => check_help(&c.attributes, &mut out),
        _ => {}
    });
    out
}

/// Calls `f` on each entry, also on those in menus, choices and `if`
/// blocks. `f` also gets whether the entry is in a choice.
fn walk<'a>(entries: &'a [Entry], in_choice: bool, f: &mut impl FnMut(&'a Entry, bool)) {
    for entry in entries {
        f(entry, in_choice);
        match entry {
            Entry::Choice(c) => walk(&c.entries, true, f),
            Entry::Menu(m) => walk(&m.entries, in_choice, f),
            Entry::If(i) => walk(&i.entries, in_choice, f),
            _ => {}
        }
    }
}

fn is_prompt(attr: &Attribute) -> bool {
    match attr {
        Attribute::Type(t) => t.prompt.is_some(),
        Attribute::Prompt(_) => true,
        _ => false,
    }
}

/// A symbol needs a type in one of its definitions, which can be in other
/// files.
fn check_type(
    index: &WorldIndex,
    c: &ConfigEntry,
    in_choice: bool,
    out: &mut Vec<ParseDiagnostic>,
) {
    // A parse error can leave the name empty. Zephyr makes some typed
    // definitions at build time, or names them with macros in templates.
    // A choice gives its type to a member without one.
    if c.name.is_empty() || index.settings.zephyr_extensions || in_choice {
        return;
    }
    if index
        .get_definitions(&c.name)
        .iter()
        .all(|d| d.type_kind.is_none())
    {
        let message = format!("no definition of `{}` gives it a type", c.name);
        push(out, c.name_span, &message, DiagSeverity::Warning);
    }
}

/// The type of a symbol, if its definitions give one and do not disagree.
fn symbol_type(index: &WorldIndex, name: &str) -> Option<TypeKind> {
    let mut types = index
        .get_definitions(name)
        .iter()
        .filter_map(|d| d.type_kind);
    let first = types.next()?;
    types.all(|t| t == first).then_some(first)
}

/// `select` and `imply` work only from and to bool or tristate symbols.
fn check_select(index: &WorldIndex, c: &ConfigEntry, out: &mut Vec<ParseDiagnostic>) {
    let is_bool = |t: &TypeKind| matches!(t, TypeKind::Bool | TypeKind::Tristate);
    let own = symbol_type(index, &c.name).filter(|t| !is_bool(t));
    for attr in &c.attributes {
        let (keyword, s) = match attr {
            Attribute::Select(s) => ("select", s),
            Attribute::Imply(s) => ("imply", s),
            _ => continue,
        };
        let (name, t, span) = if let Some(t) = own {
            (&c.name, t, s.span)
        } else if let Some(t) = symbol_type(index, &s.symbol).filter(|t| !is_bool(t)) {
            (&s.symbol, t, s.symbol_span)
        } else {
            continue;
        };
        let message = format!(
            "`{name}` is {}, but `{keyword}` works only with bool or tristate symbols",
            t.as_str()
        );
        push(out, span, &message, DiagSeverity::Warning);
    }
}

/// `range` works only for int or hex symbols.
fn check_range(index: &WorldIndex, c: &ConfigEntry, out: &mut Vec<ParseDiagnostic>) {
    let Some(t) = symbol_type(index, &c.name).filter(|t| !is_number(t)) else {
        return;
    };
    for attr in &c.attributes {
        if let Attribute::Range(r) = attr {
            let message = format!(
                "`{}` is {}, but `range` works only with int or hex symbols",
                c.name,
                t.as_str()
            );
            push(out, r.span, &message, DiagSeverity::Warning);
        }
    }
}

fn is_number(t: &TypeKind) -> bool {
    matches!(t, TypeKind::Int | TypeKind::Hex)
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
