#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::ast::*;
use crate::lexer::Lexer;
use crate::parser;

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
    pub all_symbols: Vec<String>,
    pub files: HashMap<PathBuf, FileAnalysis>,
}

impl WorldIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn analyze_file(&mut self, path: &Path, source: &str) {
        let tokens = Lexer::new(source).tokenize();
        let result = parser::parse(source, tokens);
        let line_index = LineIndex::new(source);

        let file_path = path.to_path_buf();
        let mut defs = Vec::new();
        let mut refs = Vec::new();

        collect_entries(&result.file.entries, &file_path, &mut defs, &mut refs);

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
        self.files.remove(path);

        self.definitions.retain(|_, defs| {
            defs.retain(|d| d.file != path);
            !defs.is_empty()
        });
        self.references.retain(|_, refs| {
            refs.retain(|r| r.file != path);
            !refs.is_empty()
        });
        self.all_symbols = self.definitions.keys().cloned().collect();
    }

    pub fn reanalyze_file(&mut self, path: &Path, source: &str) {
        self.remove_file(path);
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
}

fn collect_entries(
    entries: &[Entry],
    file: &Path,
    defs: &mut Vec<SymbolDef>,
    refs: &mut Vec<SymbolRef>,
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
            Entry::Choice(ch) => {
                for attr in &ch.attributes {
                    collect_attr_refs(attr, file, refs);
                }
                collect_entries(&ch.entries, file, defs, refs);
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
                collect_entries(&m.entries, file, defs, refs);
            }
            Entry::If(i) => {
                collect_expr_refs(&i.condition, RefKind::IfCondition, file, refs);
                collect_entries(&i.entries, file, defs, refs);
            }
            Entry::Source(_) | Entry::MainMenu(_) => {}
        }
    }
}

fn collect_attr_refs(attr: &Attribute, file: &Path, refs: &mut Vec<SymbolRef>) {
    match attr {
        Attribute::DependsOn(d) => {
            collect_expr_refs(&d.expr, RefKind::DependsOn, file, refs);
        }
        Attribute::Select(s) => {
            refs.push(SymbolRef {
                name: s.symbol.clone(),
                kind: RefKind::Select,
                span: s.symbol_span,
                file: file.to_path_buf(),
            });
            if let Some(cond) = &s.condition {
                collect_expr_refs(cond, RefKind::Select, file, refs);
            }
        }
        Attribute::Imply(i) => {
            refs.push(SymbolRef {
                name: i.symbol.clone(),
                kind: RefKind::Imply,
                span: i.symbol_span,
                file: file.to_path_buf(),
            });
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
            if let Some(p) = &t.prompt {
                if let Some(cond) = &p.condition {
                    collect_expr_refs(cond, RefKind::DependsOn, file, refs);
                }
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
        if is_tristate_literal(&name) || name.is_empty() {
            continue;
        }
        refs.push(SymbolRef {
            name,
            kind,
            span,
            file: file.to_path_buf(),
        });
    }
}

fn is_tristate_literal(s: &str) -> bool {
    matches!(s, "y" | "n" | "m")
}
