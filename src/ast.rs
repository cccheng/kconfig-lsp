#![allow(dead_code)]

/// Byte-offset span in source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn merge(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

/// Line-offset lookup table for converting byte offsets to (line, col).
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offset of the start of each line.
    line_starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![0];
        for (i, ch) in text.char_indices() {
            if ch == '\n' {
                line_starts.push(i + 1);
            }
        }
        Self { line_starts }
    }

    /// Convert byte offset to 0-based (line, col).
    pub fn line_col(&self, offset: usize) -> (u32, u32) {
        let line = self
            .line_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1);
        let col = offset - self.line_starts[line];
        (line as u32, col as u32)
    }

    /// Convert 0-based (line, col) to byte offset.
    pub fn offset(&self, line: u32, col: u32) -> usize {
        let line = line as usize;
        if line < self.line_starts.len() {
            self.line_starts[line] + col as usize
        } else {
            self.line_starts.last().copied().unwrap_or(0)
        }
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }
}

// ---------------------------------------------------------------------------
// AST node types – mirrors the full Kconfig grammar from
// Documentation/kbuild/kconfig-language.rst
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct KconfigFile {
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone)]
pub enum Entry {
    Config(ConfigEntry),
    MenuConfig(ConfigEntry),
    Choice(ChoiceEntry),
    Comment(CommentEntry),
    Menu(MenuEntry),
    If(IfEntry),
    Source(SourceEntry),
    MainMenu(MainMenuEntry),
}

/// Shared between `config` and `menuconfig`.
#[derive(Debug, Clone)]
pub struct ConfigEntry {
    pub name: String,
    pub name_span: Span,
    pub attributes: Vec<Attribute>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Attribute {
    Type(TypeAttr),
    Prompt(PromptAttr),
    Default(DefaultAttr),
    DefType(DefTypeAttr),
    DependsOn(DependsOnAttr),
    Select(SelectImplyAttr),
    Imply(SelectImplyAttr),
    VisibleIf(VisibleIfAttr),
    Range(RangeAttr),
    Help(HelpAttr),
    Modules(Span),
    Transitional(Span),
    Optional(Span),
}

#[derive(Debug, Clone)]
pub struct TypeAttr {
    pub kind: TypeKind,
    pub prompt: Option<PromptAttr>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    Bool,
    Tristate,
    String,
    Hex,
    Int,
}

impl TypeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TypeKind::Bool => "bool",
            TypeKind::Tristate => "tristate",
            TypeKind::String => "string",
            TypeKind::Hex => "hex",
            TypeKind::Int => "int",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PromptAttr {
    pub text: String,
    pub text_span: Span,
    pub condition: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct DefaultAttr {
    pub value: Expr,
    pub condition: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct DefTypeAttr {
    pub kind: TypeKind,
    pub value: Expr,
    pub condition: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct DependsOnAttr {
    pub expr: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct SelectImplyAttr {
    pub symbol: String,
    pub symbol_span: Span,
    pub condition: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct VisibleIfAttr {
    pub expr: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct RangeAttr {
    pub low: Expr,
    pub high: Expr,
    pub condition: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HelpAttr {
    pub text: String,
    pub span: Span,
}

// -- Compound entries -------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ChoiceEntry {
    pub attributes: Vec<Attribute>,
    pub entries: Vec<Entry>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct CommentEntry {
    pub prompt: String,
    pub prompt_span: Span,
    pub attributes: Vec<Attribute>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct MenuEntry {
    pub prompt: String,
    pub prompt_span: Span,
    pub attributes: Vec<Attribute>,
    pub entries: Vec<Entry>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct IfEntry {
    pub condition: Expr,
    pub entries: Vec<Entry>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct SourceEntry {
    pub path: String,
    pub path_span: Span,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct MainMenuEntry {
    pub prompt: String,
    pub prompt_span: Span,
    pub span: Span,
}

// -- Expressions ------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Expr {
    Symbol(String, Span),
    StringLit(String, Span),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Eq(Box<Expr>, Box<Expr>),
    NotEq(Box<Expr>, Box<Expr>),
    Less(Box<Expr>, Box<Expr>),
    LessEq(Box<Expr>, Box<Expr>),
    Greater(Box<Expr>, Box<Expr>),
    GreaterEq(Box<Expr>, Box<Expr>),
    Paren(Box<Expr>),
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Symbol(_, s) | Expr::StringLit(_, s) => *s,
            Expr::Not(e) | Expr::Paren(e) => e.span(),
            Expr::And(a, b)
            | Expr::Or(a, b)
            | Expr::Eq(a, b)
            | Expr::NotEq(a, b)
            | Expr::Less(a, b)
            | Expr::LessEq(a, b)
            | Expr::Greater(a, b)
            | Expr::GreaterEq(a, b) => a.span().merge(b.span()),
        }
    }

    /// Collect all symbol references inside this expression.
    pub fn collect_symbols(&self, out: &mut Vec<(String, Span)>) {
        match self {
            Expr::Symbol(name, span) => out.push((name.clone(), *span)),
            Expr::StringLit(..) => {}
            Expr::Not(e) | Expr::Paren(e) => e.collect_symbols(out),
            Expr::And(a, b)
            | Expr::Or(a, b)
            | Expr::Eq(a, b)
            | Expr::NotEq(a, b)
            | Expr::Less(a, b)
            | Expr::LessEq(a, b)
            | Expr::Greater(a, b)
            | Expr::GreaterEq(a, b) => {
                a.collect_symbols(out);
                b.collect_symbols(out);
            }
        }
    }
}

// -- Parse diagnostics (errors / warnings) ----------------------------------

#[derive(Debug, Clone)]
pub struct ParseDiagnostic {
    pub message: String,
    pub span: Span,
    pub severity: DiagSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagSeverity {
    Error,
    Warning,
}
