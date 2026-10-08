use std::path::{Component, Path, PathBuf};

use crate::analysis::WorldIndex;
use crate::ast::{Entry, KconfigFile, SourceEntry, Variable};
use crate::settings::glob_match;

/// The deepest that variables can refer to other variables. This stops a
/// variable that refers to itself.
const MAX_DEPTH: usize = 10;

/// What sets the files that the `source` statements of a file name: the
/// paths of the statements and the macro variables of the file.
#[derive(PartialEq)]
pub struct SourceKey {
    paths: Vec<(String, bool)>,
    variables: Vec<Variable>,
}

pub fn source_key(file: &KconfigFile) -> SourceKey {
    SourceKey {
        paths: source_entries(file)
            .into_iter()
            .map(|e| (e.path.clone(), e.relative))
            .collect(),
        variables: file.variables.clone(),
    }
}

/// The `source` statements of a file, also those in menus, choices and
/// `if` blocks.
pub fn source_entries(file: &KconfigFile) -> Vec<&SourceEntry> {
    let mut out = Vec::new();
    collect(&file.entries, &mut out);
    out
}

fn collect<'a>(entries: &'a [Entry], out: &mut Vec<&'a SourceEntry>) {
    for entry in entries {
        match entry {
            Entry::Source(s) => out.push(s),
            Entry::Menu(m) => collect(&m.entries, out),
            Entry::Choice(c) => collect(&c.entries, out),
            Entry::If(i) => collect(&i.entries, out),
            _ => {}
        }
    }
}

/// The files that a `source` statement in `file` names, sorted. The
/// variables in the path come from `variables`, the macro variables of
/// `file`, and then from the environment. `root` is the top directory of
/// the tree.
pub fn resolve(
    entry: &SourceEntry,
    file: &Path,
    variables: &[Variable],
    root: Option<&Path>,
) -> Vec<PathBuf> {
    resolve_with(entry, file, variables, root, &|name| {
        std::env::var(name).ok()
    })
}

/// Like `resolve`, but takes the environment from `env`.
pub fn resolve_with(
    entry: &SourceEntry,
    file: &Path,
    variables: &[Variable],
    root: Option<&Path>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Vec<PathBuf> {
    let Some(path) = expand(&entry.path, variables, env, 0) else {
        return Vec::new();
    };
    let path = Path::new(&path);
    let dir = file.parent().unwrap_or(Path::new(""));
    if path.is_absolute() || entry.relative {
        return glob(&dir.join(path));
    }
    // Most trees give the path from the top directory. Some, such as the
    // boards of RT-Thread, give it from the file with the `source`.
    if let Some(root) = root {
        let files = glob(&root.join(path));
        if !files.is_empty() {
            return files;
        }
    }
    glob(&dir.join(path))
}

/// Reads the files that the `source` statements in `paths` name, and the
/// files that those files name in turn. Reads only the files in the
/// workspace that are not in the index. Returns, sorted, all files in the
/// workspace that the statements name, also those that were in the index
/// already.
pub fn read_sourced_files(index: &mut WorldIndex, paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let Some(root) = index.root.clone() else {
        return Vec::new();
    };
    let mut named = Vec::new();
    let mut todo = paths;
    while let Some(path) = todo.pop() {
        let Some(fa) = index.files.get(&path) else {
            continue;
        };
        let files: Vec<PathBuf> = source_entries(&fa.file)
            .into_iter()
            .flat_map(|e| resolve(e, &path, &fa.file.variables, Some(&root)))
            .collect();
        for file in files {
            if !file.starts_with(&root) {
                continue;
            }
            if !index.files.contains_key(&file) {
                let Ok(source) = std::fs::read_to_string(&file) else {
                    continue;
                };
                index.analyze_file(&file, &source);
                todo.push(file.clone());
            }
            named.push(file);
        }
    }
    named.sort();
    named.dedup();
    named
}

/// Expands `$(NAME)`, `${NAME}` and `$NAME` in `text`. Returns `None` if a
/// name has no value, or if `text` calls a macro function.
fn expand(
    text: &str,
    variables: &[Variable],
    env: &dyn Fn(&str) -> Option<String>,
    depth: usize,
) -> Option<String> {
    if depth > MAX_DEPTH {
        return None;
    }
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find('$') {
        out.push_str(&rest[..i]);
        rest = &rest[i + 1..];
        let (name, after) = if let Some(r) = rest.strip_prefix('(') {
            let end = closing_paren(r)?;
            // The name can also have variables, as in `$(ARCH_$(BITS))`.
            (expand(&r[..end], variables, env, depth + 1)?, &r[end + 1..])
        } else if let Some(r) = rest.strip_prefix('{') {
            let end = r.find('}')?;
            (r[..end].to_string(), &r[end + 1..])
        } else {
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            if end == 0 {
                out.push('$');
                continue;
            }
            (rest[..end].to_string(), &rest[end..])
        };
        // A function call has arguments, as in `$(shell,cmd)`.
        if name.is_empty() || name.contains(|c: char| c == ',' || c.is_whitespace()) {
            return None;
        }
        out.push_str(&lookup(&name, variables, env, depth)?);
        rest = after;
    }
    out.push_str(rest);
    Some(out)
}

/// The value of a variable: its last assignment in the file, else the
/// environment.
fn lookup(
    name: &str,
    variables: &[Variable],
    env: &dyn Fn(&str) -> Option<String>,
    depth: usize,
) -> Option<String> {
    let mut value: Option<String> = None;
    for v in variables.iter().filter(|v| v.name == name) {
        value = Some(match value {
            Some(old) if v.append => format!("{old} {}", v.value),
            _ => v.value.clone(),
        });
    }
    match value {
        Some(value) => expand(&value, variables, env, depth + 1),
        None => env(name),
    }
}

/// The position of the `)` that closes a `(` just before `text`.
fn closing_paren(text: &str) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return Some(i),
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Removes `.` and `..` from a path without reading the file system.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                // There is nothing above the top directory.
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => out.push(".."),
            },
            _ => out.push(c),
        }
    }
    out
}

/// The regular files that `path` names, sorted. `*` and `?` in a component
/// match the names in its directory. As in the system, `..` goes up from
/// the directory that the path names so far, which must exist. Only the
/// found files lose their `.` and `..`.
fn glob(path: &Path) -> Vec<PathBuf> {
    let mut found = vec![PathBuf::new()];
    for c in path.components() {
        let pattern = match c {
            Component::Normal(name) => name.to_str().filter(|n| n.contains(['*', '?'])),
            _ => None,
        };
        match pattern {
            Some(pattern) => found = found.iter().flat_map(|dir| matches(dir, pattern)).collect(),
            None => found.iter_mut().for_each(|p| p.push(c)),
        }
    }
    let mut files: Vec<PathBuf> = found
        .into_iter()
        .filter(|p| p.is_file())
        .map(|p| normalize(&p))
        .collect();
    files.sort();
    files.dedup();
    files
}

/// The entries of `dir` whose names match `pattern`.
fn matches(dir: &Path, pattern: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            // As in a shell, a pattern matches a name that starts with `.`
            // only if the pattern starts with `.` too.
            (pattern.starts_with('.') || !name.starts_with('.')) && glob_match(pattern, &name)
        })
        .map(|e| e.path())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var(name: &str, append: bool, value: &str) -> Variable {
        Variable {
            name: name.to_string(),
            append,
            value: value.to_string(),
        }
    }

    fn env(name: &str) -> Option<String> {
        (name == "SRCARCH").then(|| "arm".to_string())
    }

    #[test]
    fn expand_takes_file_variables_then_the_environment() {
        let vars = [
            var("RTT_DIR", false, "../.."),
            var("SRCARCH", false, "x86"),
            var("FLAGS", false, "a"),
            var("FLAGS", true, "b"),
            var("TOP", false, "$(RTT_DIR)/top"),
        ];
        let cases = [
            ("arch/$(SRCARCH)/Kconfig", Some("arch/x86/Kconfig")),
            ("$(RTT_DIR)/Kconfig", Some("../../Kconfig")),
            ("${RTT_DIR}/$RTT_DIR", Some("../../../..")),
            ("$(TOP)", Some("../../top")),
            ("$(FLAGS)", Some("a b")),
            ("$(ARCH_$(SRCARCH))", None),
            ("$(UNKNOWN)/Kconfig", None),
            ("$(shell,ls)", None),
            ("$(info x)", None),
            ("$(SRCARCH", None),
            ("a$/b", Some("a$/b")),
        ];
        for (text, want) in cases {
            let got = expand(text, &vars, &env, 0);
            assert_eq!(got.as_deref(), want, "{text}");
        }
        assert_eq!(
            expand("arch/$(SRCARCH)", &[], &env, 0).as_deref(),
            Some("arch/arm")
        );
    }

    #[test]
    fn expand_stops_at_a_variable_that_refers_to_itself() {
        let vars = [var("A", false, "$(B)"), var("B", false, "$(A)")];
        assert_eq!(expand("$(A)", &vars, &env, 0), None);
    }

    #[test]
    fn normalize_removes_dots() {
        let cases = [
            ("/a/b/../../c/./d", "/c/d"),
            ("/../a", "/a"),
            ("a/../../b", "../b"),
            ("../../a", "../../a"),
            ("./a/./b", "a/b"),
        ];
        for (path, want) in cases {
            assert_eq!(normalize(Path::new(path)), Path::new(want), "{path}");
        }
    }
}
