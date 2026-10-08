#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::ast::*;
use crate::lexer::Lexer;
use crate::parser;
use crate::settings::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefKind {
    Config,
    MenuConfig,
    Choice,
}

#[derive(Debug, Clone)]
pub struct SymbolDef {
    pub name: String,
    pub kind: DefKind,
    pub name_span: Span,
    pub type_kind: Option<TypeKind>,
    pub prompt: Option<String>,
    pub help: Option<String>,
    pub file: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    DependsOn,
    Select,
    Imply,
    Default,
    Range,
    VisibleIf,
    IfCondition,
}

#[derive(Debug, Clone)]
pub struct SymbolRef {
    pub name: String,
    pub kind: RefKind,
    pub span: Span,
    pub file: PathBuf,
}

/// A `configdefault` block, kept so hover can show the defaults it adds
/// and goto can jump to it.
#[derive(Debug, Clone)]
pub struct ConfigDefaultInfo {
    pub name: String,
    pub name_span: Span,
    /// Span of each `default` line in the block.
    pub defaults: Vec<Span>,
    pub file: PathBuf,
}

#[derive(Debug, Clone)]
pub struct FileAnalysis {
    pub file: KconfigFile,
    pub line_index: LineIndex,
    pub source: String,
    pub diagnostics: Vec<ParseDiagnostic>,
}

#[derive(Debug, Default)]
pub struct WorldIndex {
    pub definitions: HashMap<String, Vec<SymbolDef>>,
    pub references: HashMap<String, Vec<SymbolRef>>,
    pub configdefaults: HashMap<String, Vec<ConfigDefaultInfo>>,
    pub all_symbols: Vec<String>,
    pub files: HashMap<PathBuf, FileAnalysis>,
    pub settings: Settings,
    /// The top directory of the workspace. Most `source` paths start there.
    pub root: Option<PathBuf>,
}

impl WorldIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Analyzes a file and adds it to the index. An analysis of the file
    /// that is in the index already leaves first, also if `path` spells the
    /// path in another way, such as with `//`.
    pub fn analyze_file(&mut self, path: &Path, source: &str) {
        self.remove_file(path);
        let tokens = Lexer::new(source, &self.settings).tokenize();
        let result = parser::parse(source, tokens);
        let line_index = LineIndex::new(source);

        let file_path = path.to_path_buf();
        let mut defs = Vec::new();
        let mut refs = Vec::new();
        let mut configdefaults = Vec::new();

        collect_entries(
            &result.file.entries,
            &file_path,
            &mut defs,
            &mut refs,
            &mut configdefaults,
        );

        for d in &defs {
            self.definitions
                .entry(d.name.clone())
                .or_default()
                .push(d.clone());
            if !self.all_symbols.contains(&d.name) {
                self.all_symbols.push(d.name.clone());
            }
        }
        for r in &refs {
            self.references
                .entry(r.name.clone())
                .or_default()
                .push(r.clone());
        }
        for info in configdefaults {
            self.configdefaults
                .entry(info.name.clone())
                .or_default()
                .push(info);
        }

        self.files.insert(
            file_path,
            FileAnalysis {
                file: result.file,
                line_index,
                source: source.to_string(),
                diagnostics: result.diagnostics,
            },
        );
    }

    pub fn remove_file(&mut self, path: &Path) {
        let Some((path, _)) = self.files.remove_entry(path) else {
            return;
        };
        // The definitions and references of the file have the path of its
        // key. Comparing the bytes is much faster than comparing `Path`
        // components, and each change to a file removes it.
        let path = path.as_os_str();
        self.definitions.retain(|_, defs| {
            defs.retain(|d| d.file.as_os_str() != path);
            !defs.is_empty()
        });
        self.references.retain(|_, refs| {
            refs.retain(|r| r.file.as_os_str() != path);
            !refs.is_empty()
        });
        self.configdefaults.retain(|_, infos| {
            infos.retain(|info| info.file.as_os_str() != path);
            !infos.is_empty()
        });
        self.all_symbols = self.definitions.keys().cloned().collect();
    }

    pub fn reanalyze_file(&mut self, path: &Path, source: &str) {
        self.analyze_file(path, source);
    }

    pub fn get_definitions(&self, name: &str) -> &[SymbolDef] {
        self.definitions
            .get(name)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn get_references(&self, name: &str) -> &[SymbolRef] {
        self.references
            .get(name)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn get_configdefaults(&self, name: &str) -> &[ConfigDefaultInfo] {
        self.configdefaults
            .get(name)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

fn collect_entries(
    entries: &[Entry],
    file: &Path,
    defs: &mut Vec<SymbolDef>,
    refs: &mut Vec<SymbolRef>,
    configdefaults: &mut Vec<ConfigDefaultInfo>,
) {
    for entry in entries {
        match entry {
            Entry::Config(c) | Entry::MenuConfig(c) => {
                let kind = if matches!(entry, Entry::MenuConfig(_)) {
                    DefKind::MenuConfig
                } else {
                    DefKind::Config
                };
                let mut type_kind = None;
                let mut prompt = None;
                let mut help = None;

                for attr in &c.attributes {
                    match attr {
                        Attribute::Type(t) => {
                            type_kind = Some(t.kind);
                            if let Some(p) = &t.prompt {
                                prompt = Some(p.text.clone());
                            }
                        }
                        Attribute::DefType(dt) => {
                            type_kind = Some(dt.kind);
                        }
                        Attribute::Prompt(p) => {
                            prompt = Some(p.text.clone());
                        }
                        Attribute::Help(h) => {
                            help = Some(h.text.clone());
                        }
                        _ => {}
                    }
                    collect_attr_refs(attr, file, refs);
                }

                // A parse error can leave the name empty.
                if c.name.is_empty() {
                    continue;
                }
                defs.push(SymbolDef {
                    name: c.name.clone(),
                    kind,
                    name_span: c.name_span,
                    type_kind,
                    prompt,
                    help,
                    file: file.to_path_buf(),
                });
            }
            Entry::ConfigDefault(c) => {
                configdefaults.push(ConfigDefaultInfo {
                    name: c.name.clone(),
                    name_span: c.name_span,
                    defaults: c
                        .attributes
                        .iter()
                        .filter_map(|attr| match attr {
                            Attribute::Default(d) => Some(d.span),
                            _ => None,
                        })
                        .collect(),
                    file: file.to_path_buf(),
                });
                // configdefault is in fact a reference
                push_ref(&c.name, RefKind::Default, c.name_span, file, refs);
                // handle references used within configdefault
                for attr in &c.attributes {
                    collect_attr_refs(attr, file, refs);
                }
            }
            Entry::Choice(ch) => {
                for attr in &ch.attributes {
                    collect_attr_refs(attr, file, refs);
                }
                collect_entries(&ch.entries, file, defs, refs, configdefaults);
            }
            Entry::Comment(cm) => {
                for attr in &cm.attributes {
                    collect_attr_refs(attr, file, refs);
                }
            }
            Entry::Menu(m) => {
                for attr in &m.attributes {
                    collect_attr_refs(attr, file, refs);
                }
                collect_entries(&m.entries, file, defs, refs, configdefaults);
            }
            Entry::If(i) => {
                collect_expr_refs(&i.condition, RefKind::IfCondition, file, refs);
                collect_entries(&i.entries, file, defs, refs, configdefaults);
            }
            Entry::Source(_) | Entry::MainMenu(_) => {}
        }
    }
}

fn collect_attr_refs(attr: &Attribute, file: &Path, refs: &mut Vec<SymbolRef>) {
    match attr {
        Attribute::DependsOn(d) => {
            collect_expr_refs(&d.expr, RefKind::DependsOn, file, refs);
            if let Some(cond) = &d.condition {
                collect_expr_refs(cond, RefKind::DependsOn, file, refs);
            }
        }
        Attribute::Select(s) => {
            push_ref(&s.symbol, RefKind::Select, s.symbol_span, file, refs);
            if let Some(cond) = &s.condition {
                collect_expr_refs(cond, RefKind::Select, file, refs);
            }
        }
        Attribute::Imply(i) => {
            push_ref(&i.symbol, RefKind::Imply, i.symbol_span, file, refs);
            if let Some(cond) = &i.condition {
                collect_expr_refs(cond, RefKind::Imply, file, refs);
            }
        }
        Attribute::Default(d) => {
            collect_expr_refs(&d.value, RefKind::Default, file, refs);
            if let Some(cond) = &d.condition {
                collect_expr_refs(cond, RefKind::Default, file, refs);
            }
        }
        Attribute::DefType(dt) => {
            collect_expr_refs(&dt.value, RefKind::Default, file, refs);
            if let Some(cond) = &dt.condition {
                collect_expr_refs(cond, RefKind::Default, file, refs);
            }
        }
        Attribute::VisibleIf(v) => {
            collect_expr_refs(&v.expr, RefKind::VisibleIf, file, refs);
        }
        Attribute::Range(r) => {
            collect_expr_refs(&r.low, RefKind::Range, file, refs);
            collect_expr_refs(&r.high, RefKind::Range, file, refs);
            if let Some(cond) = &r.condition {
                collect_expr_refs(cond, RefKind::Range, file, refs);
            }
        }
        Attribute::Type(t) => {
            if let Some(cond) = t.prompt.as_ref().and_then(|p| p.condition.as_ref()) {
                collect_expr_refs(cond, RefKind::DependsOn, file, refs);
            }
        }
        Attribute::Prompt(p) => {
            if let Some(cond) = &p.condition {
                collect_expr_refs(cond, RefKind::DependsOn, file, refs);
            }
        }
        Attribute::Help(_)
        | Attribute::Modules(_)
        | Attribute::Transitional(_)
        | Attribute::Optional(_) => {}
    }
}

fn collect_expr_refs(expr: &Expr, kind: RefKind, file: &Path, refs: &mut Vec<SymbolRef>) {
    let mut syms = Vec::new();
    expr.collect_symbols(&mut syms);
    for (name, span) in syms {
        if !is_tristate_literal(&name) && !is_numeric_literal(&name) {
            push_ref(&name, kind, span, file, refs);
        }
    }
}

/// Records a reference. A parse error can leave the name empty, and an
/// empty name refers to no symbol, so skip it.
fn push_ref(name: &str, kind: RefKind, span: Span, file: &Path, refs: &mut Vec<SymbolRef>) {
    if name.is_empty() {
        return;
    }
    refs.push(SymbolRef {
        name: name.to_string(),
        kind,
        span,
        file: file.to_path_buf(),
    });
}

pub(crate) fn is_tristate_literal(s: &str) -> bool {
    matches!(s, "y" | "n" | "m")
}

pub(crate) fn is_numeric_literal(s: &str) -> bool {
    if s.starts_with("0x") || s.starts_with("0X") {
        s.len() > 2 && s[2..].chars().all(|c| c.is_ascii_hexdigit())
    } else {
        !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
    }
}
