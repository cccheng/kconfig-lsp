use crate::ast::*;
use crate::lexer::{Token, TokenKind};

pub struct ParseResult {
    pub file: KconfigFile,
    pub diagnostics: Vec<ParseDiagnostic>,
}

pub fn parse(source: &str, tokens: Vec<Token>) -> ParseResult {
    let mut p = Parser {
        source,
        tokens,
        pos: 0,
        diagnostics: Vec::new(),
    };
    let entries = p.parse_entries(&[]);
    ParseResult {
        file: KconfigFile { entries },
        diagnostics: p.diagnostics,
    }
}

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: Vec<ParseDiagnostic>,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> &TokenKind {
        self.tokens
            .get(self.pos)
            .map(|t| &t.kind)
            .unwrap_or(&TokenKind::Eof)
    }

    fn current_span(&self) -> Span {
        self.tokens
            .get(self.pos)
            .map(|t| t.span)
            .unwrap_or(Span::new(self.source.len(), self.source.len()))
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), TokenKind::Newline | TokenKind::LineComment(_)) {
            self.pos += 1;
        }
    }

    fn skip_to_eol(&mut self) {
        while !matches!(self.peek(), TokenKind::Newline | TokenKind::Eof) {
            self.pos += 1;
        }
        if *self.peek() == TokenKind::Newline {
            self.pos += 1;
        }
    }

    fn expect_newline(&mut self) {
        match self.peek() {
            TokenKind::Newline => {
                self.pos += 1;
            }
            TokenKind::LineComment(_) => {
                self.pos += 1;
                if *self.peek() == TokenKind::Newline {
                    self.pos += 1;
                }
            }
            TokenKind::Eof => {}
            _ => {
                self.diag(
                    self.current_span(),
                    "expected end of line",
                    DiagSeverity::Warning,
                );
                self.skip_to_eol();
            }
        }
    }

    fn diag(&mut self, span: Span, msg: &str, severity: DiagSeverity) {
        self.diagnostics.push(ParseDiagnostic {
            message: msg.to_string(),
            span,
            severity,
        });
    }

    // -----------------------------------------------------------------------
    // Entry parsing – handles the block structure of Kconfig
    // -----------------------------------------------------------------------

    fn parse_entries(&mut self, terminators: &[TokenKind]) -> Vec<Entry> {
        let mut entries = Vec::new();
        loop {
            self.skip_newlines();
            if *self.peek() == TokenKind::Eof {
                break;
            }
            if terminators.iter().any(|t| t == self.peek()) {
                break;
            }
            if let Some(entry) = self.parse_entry() {
                entries.push(entry);
            }
        }
        entries
    }

    fn parse_entry(&mut self) -> Option<Entry> {
        match self.peek().clone() {
            TokenKind::Config => Some(self.parse_config(false)),
            TokenKind::MenuConfig => Some(self.parse_config(true)),
            TokenKind::Choice => Some(self.parse_choice()),
            TokenKind::CommentKw => Some(self.parse_comment()),
            TokenKind::Menu => Some(self.parse_menu()),
            TokenKind::If => Some(self.parse_if()),
            TokenKind::Source => Some(self.parse_source()),
            TokenKind::MainMenu => Some(self.parse_mainmenu()),
            _ => {
                let span = self.current_span();
                self.diag(span, "unexpected token at top level", DiagSeverity::Error);
                self.skip_to_eol();
                None
            }
        }
    }

    // -----------------------------------------------------------------------
    // config / menuconfig
    // -----------------------------------------------------------------------

    fn parse_config(&mut self, is_menuconfig: bool) -> Entry {
        let start_span = self.current_span();
        self.pos += 1; // skip `config` / `menuconfig`

        let (name, name_span) = self.expect_ident();
        self.expect_newline();

        let attributes = self.parse_config_attributes();
        let span = start_span.merge(attributes.last().map(attr_span).unwrap_or(name_span));

        let entry = ConfigEntry {
            name,
            name_span,
            attributes,
            span,
        };
        if is_menuconfig {
            Entry::MenuConfig(entry)
        } else {
            Entry::Config(entry)
        }
    }

    fn parse_config_attributes(&mut self) -> Vec<Attribute> {
        let mut attrs = Vec::new();
        loop {
            self.skip_newlines();
            match self.peek() {
                TokenKind::Bool
                | TokenKind::Tristate
                | TokenKind::StringType
                | TokenKind::Hex
                | TokenKind::Int => attrs.push(self.parse_type_attr()),

                TokenKind::Prompt => attrs.push(self.parse_prompt_attr()),
                TokenKind::Default => attrs.push(self.parse_default_attr()),
                TokenKind::DefBool => attrs.push(self.parse_def_type_attr(TypeKind::Bool)),
                TokenKind::DefTristate => attrs.push(self.parse_def_type_attr(TypeKind::Tristate)),
                TokenKind::Depends => attrs.push(self.parse_depends_on()),
                TokenKind::Select => attrs.push(self.parse_select_imply(true)),
                TokenKind::Imply => attrs.push(self.parse_select_imply(false)),
                TokenKind::Visible => attrs.push(self.parse_visible_if()),
                TokenKind::Range => attrs.push(self.parse_range()),
                TokenKind::Help => attrs.push(self.parse_help()),
                TokenKind::Modules => {
                    let span = self.current_span();
                    self.pos += 1;
                    self.expect_newline();
                    attrs.push(Attribute::Modules(span));
                }
                TokenKind::Transitional => {
                    let span = self.current_span();
                    self.pos += 1;
                    self.expect_newline();
                    attrs.push(Attribute::Transitional(span));
                }
                TokenKind::Optional => {
                    let span = self.current_span();
                    self.pos += 1;
                    self.expect_newline();
                    attrs.push(Attribute::Optional(span));
                }
                _ => break,
            }
        }
        attrs
    }

    // -----------------------------------------------------------------------
    // Attribute parsers
    // -----------------------------------------------------------------------

    fn parse_type_attr(&mut self) -> Attribute {
        let start = self.current_span();
        let kind = self.consume_type_kind();
        let prompt = self.try_parse_inline_prompt();
        let span = start.merge(prompt.as_ref().map(|p| p.span).unwrap_or(start));
        self.expect_newline();
        Attribute::Type(TypeAttr { kind, prompt, span })
    }

    fn parse_prompt_attr(&mut self) -> Attribute {
        let start = self.current_span();
        self.pos += 1; // skip `prompt`
        let prompt = self.parse_prompt_value(start);
        self.expect_newline();
        Attribute::Prompt(prompt)
    }

    fn parse_default_attr(&mut self) -> Attribute {
        let start = self.current_span();
        self.pos += 1; // skip `default`
        let value = self.parse_expr();
        let condition = self.try_parse_if_condition();
        let span = start.merge(condition.as_ref().map(|e| e.span()).unwrap_or(value.span()));
        self.expect_newline();
        Attribute::Default(DefaultAttr {
            value,
            condition,
            span,
        })
    }

    fn parse_def_type_attr(&mut self, kind: TypeKind) -> Attribute {
        let start = self.current_span();
        self.pos += 1;
        let value = self.parse_expr();
        let condition = self.try_parse_if_condition();
        let span = start.merge(condition.as_ref().map(|e| e.span()).unwrap_or(value.span()));
        self.expect_newline();
        Attribute::DefType(DefTypeAttr {
            kind,
            value,
            condition,
            span,
        })
    }

    fn parse_depends_on(&mut self) -> Attribute {
        let start = self.current_span();
        self.pos += 1; // skip `depends`
        if *self.peek() == TokenKind::On {
            self.pos += 1;
        }
        let expr = self.parse_expr();
        let span = start.merge(expr.span());
        self.expect_newline();
        Attribute::DependsOn(DependsOnAttr { expr, span })
    }

    fn parse_select_imply(&mut self, is_select: bool) -> Attribute {
        let start = self.current_span();
        self.pos += 1;
        let (symbol, symbol_span) = self.expect_ident();
        let condition = self.try_parse_if_condition();
        let span = start.merge(condition.as_ref().map(|e| e.span()).unwrap_or(symbol_span));
        self.expect_newline();
        let attr = SelectImplyAttr {
            symbol,
            symbol_span,
            condition,
            span,
        };
        if is_select {
            Attribute::Select(attr)
        } else {
            Attribute::Imply(attr)
        }
    }

    fn parse_visible_if(&mut self) -> Attribute {
        let start = self.current_span();
        self.pos += 1; // skip `visible`
        if *self.peek() == TokenKind::If {
            self.pos += 1;
        }
        let expr = self.parse_expr();
        let span = start.merge(expr.span());
        self.expect_newline();
        Attribute::VisibleIf(VisibleIfAttr { expr, span })
    }

    fn parse_range(&mut self) -> Attribute {
        let start = self.current_span();
        self.pos += 1; // skip `range`
        let low = self.parse_primary_expr();
        let high = self.parse_primary_expr();
        let condition = self.try_parse_if_condition();
        let span = start.merge(condition.as_ref().map(|e| e.span()).unwrap_or(high.span()));
        self.expect_newline();
        Attribute::Range(RangeAttr {
            low,
            high,
            condition,
            span,
        })
    }

    fn parse_help(&mut self) -> Attribute {
        let start = self.current_span();
        self.pos += 1; // skip `help`
        self.skip_to_eol();

        let help_text = self.consume_help_text();
        let end_offset = start.end + help_text.len();
        Attribute::Help(HelpAttr {
            text: help_text,
            span: start.merge(Span::new(start.start, end_offset)),
        })
    }

    fn consume_help_text(&mut self) -> String {
        let mut lines: Vec<&str> = Vec::new();
        let mut base_indent: Option<usize> = None;

        let src = self.source;

        let token_offset = self.current_span().start;
        let raw_start = src[..token_offset]
            .rfind('\n')
            .map_or(token_offset, |p| p + 1);
        let remaining = &src[raw_start..];

        let mut consumed = 0usize;
        for raw_line in remaining.lines() {
            let trimmed = raw_line.trim_start();
            if trimmed.is_empty() {
                lines.push("");
                consumed += raw_line.len() + 1;
                continue;
            }
            let indent = raw_line.len() - trimmed.len();
            match base_indent {
                None => {
                    base_indent = Some(indent);
                }
                Some(bi) => {
                    if indent < bi {
                        break;
                    }
                }
            }
            lines.push(raw_line);
            consumed += raw_line.len() + 1;
        }

        // Advance the token stream past the consumed help text.
        let end_offset = raw_start + consumed;
        while self.pos < self.tokens.len() {
            if self.tokens[self.pos].span.start >= end_offset {
                break;
            }
            self.pos += 1;
        }

        // Strip the base indent from each line.
        let bi = base_indent.unwrap_or(0);
        lines
            .iter()
            .map(|l| {
                if l.len() > bi {
                    &l[bi..]
                } else {
                    l.trim_start()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end()
            .to_string()
    }

    // -----------------------------------------------------------------------
    // Compound entries
    // -----------------------------------------------------------------------

    fn parse_choice(&mut self) -> Entry {
        let start = self.current_span();
        self.pos += 1; // skip `choice`
        self.expect_newline();

        let mut attributes = Vec::new();
        loop {
            self.skip_newlines();
            match self.peek() {
                TokenKind::Prompt => attributes.push(self.parse_prompt_attr()),
                TokenKind::Default => attributes.push(self.parse_default_attr()),
                TokenKind::Depends => attributes.push(self.parse_depends_on()),
                TokenKind::Help => attributes.push(self.parse_help()),
                TokenKind::Bool | TokenKind::Tristate => attributes.push(self.parse_type_attr()),
                TokenKind::Optional => {
                    let span = self.current_span();
                    self.pos += 1;
                    self.expect_newline();
                    attributes.push(Attribute::Optional(span));
                }
                _ => break,
            }
        }

        let entries = self.parse_entries(&[TokenKind::EndChoice]);
        self.skip_newlines();
        let end_span = self.current_span();
        if *self.peek() == TokenKind::EndChoice {
            self.pos += 1;
            self.expect_newline();
        } else {
            self.diag(end_span, "expected `endchoice`", DiagSeverity::Error);
        }

        Entry::Choice(ChoiceEntry {
            attributes,
            entries,
            span: start.merge(end_span),
        })
    }

    fn parse_comment(&mut self) -> Entry {
        let start = self.current_span();
        self.pos += 1; // skip `comment`
        let (prompt, prompt_span) = self.expect_string();
        self.expect_newline();

        let attributes = self.parse_comment_menu_attrs();
        let span = start.merge(attributes.last().map(attr_span).unwrap_or(prompt_span));
        Entry::Comment(CommentEntry {
            prompt,
            prompt_span,
            attributes,
            span,
        })
    }

    fn parse_menu(&mut self) -> Entry {
        let start = self.current_span();
        self.pos += 1; // skip `menu`
        let (prompt, prompt_span) = self.expect_string();
        self.expect_newline();

        let attributes = self.parse_comment_menu_attrs();
        let entries = self.parse_entries(&[TokenKind::EndMenu]);
        self.skip_newlines();
        let end_span = self.current_span();
        if *self.peek() == TokenKind::EndMenu {
            self.pos += 1;
            self.expect_newline();
        } else {
            self.diag(end_span, "expected `endmenu`", DiagSeverity::Error);
        }

        Entry::Menu(MenuEntry {
            prompt,
            prompt_span,
            attributes,
            entries,
            span: start.merge(end_span),
        })
    }

    fn parse_comment_menu_attrs(&mut self) -> Vec<Attribute> {
        let mut attrs = Vec::new();
        loop {
            self.skip_newlines();
            match self.peek() {
                TokenKind::Depends => attrs.push(self.parse_depends_on()),
                TokenKind::Visible => attrs.push(self.parse_visible_if()),
                _ => break,
            }
        }
        attrs
    }

    fn parse_if(&mut self) -> Entry {
        let start = self.current_span();
        self.pos += 1; // skip `if`
        let condition = self.parse_expr();
        self.expect_newline();

        let entries = self.parse_entries(&[TokenKind::EndIf]);
        self.skip_newlines();
        let end_span = self.current_span();
        if *self.peek() == TokenKind::EndIf {
            self.pos += 1;
            self.expect_newline();
        } else {
            self.diag(end_span, "expected `endif`", DiagSeverity::Error);
        }

        Entry::If(IfEntry {
            condition,
            entries,
            span: start.merge(end_span),
        })
    }

    fn parse_source(&mut self) -> Entry {
        let start = self.current_span();
        self.pos += 1; // skip `source`
        let (path, path_span) = self.expect_string();
        self.expect_newline();
        Entry::Source(SourceEntry {
            path,
            path_span,
            span: start.merge(path_span),
        })
    }

    fn parse_mainmenu(&mut self) -> Entry {
        let start = self.current_span();
        self.pos += 1; // skip `mainmenu`
        let (prompt, prompt_span) = self.expect_string();
        self.expect_newline();
        Entry::MainMenu(MainMenuEntry {
            prompt,
            prompt_span,
            span: start.merge(prompt_span),
        })
    }

    // -----------------------------------------------------------------------
    // Expression parser – precedence climbing
    //
    // Precedence (highest to lowest):
    //   1. primary: symbol, string, '(' expr ')', '!' expr
    //   2. comparison: =, !=, <, >, <=, >=
    //   3. AND: &&
    //   4. OR:  ||
    // -----------------------------------------------------------------------

    fn parse_expr(&mut self) -> Expr {
        self.parse_or_expr()
    }

    fn parse_or_expr(&mut self) -> Expr {
        let mut left = self.parse_and_expr();
        while *self.peek() == TokenKind::Or {
            self.pos += 1;
            let right = self.parse_and_expr();
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        left
    }

    fn parse_and_expr(&mut self) -> Expr {
        let mut left = self.parse_comparison_expr();
        while *self.peek() == TokenKind::And {
            self.pos += 1;
            let right = self.parse_comparison_expr();
            left = Expr::And(Box::new(left), Box::new(right));
        }
        left
    }

    fn parse_comparison_expr(&mut self) -> Expr {
        let left = self.parse_primary_expr();
        match self.peek().clone() {
            TokenKind::Eq => {
                self.pos += 1;
                let right = self.parse_primary_expr();
                Expr::Eq(Box::new(left), Box::new(right))
            }
            TokenKind::NotEq => {
                self.pos += 1;
                let right = self.parse_primary_expr();
                Expr::NotEq(Box::new(left), Box::new(right))
            }
            TokenKind::Less => {
                self.pos += 1;
                let right = self.parse_primary_expr();
                Expr::Less(Box::new(left), Box::new(right))
            }
            TokenKind::LessEq => {
                self.pos += 1;
                let right = self.parse_primary_expr();
                Expr::LessEq(Box::new(left), Box::new(right))
            }
            TokenKind::Greater => {
                self.pos += 1;
                let right = self.parse_primary_expr();
                Expr::Greater(Box::new(left), Box::new(right))
            }
            TokenKind::GreaterEq => {
                self.pos += 1;
                let right = self.parse_primary_expr();
                Expr::GreaterEq(Box::new(left), Box::new(right))
            }
            _ => left,
        }
    }

    fn parse_primary_expr(&mut self) -> Expr {
        match self.peek().clone() {
            TokenKind::Not => {
                self.pos += 1;
                let inner = self.parse_primary_expr();
                Expr::Not(Box::new(inner))
            }
            TokenKind::OpenParen => {
                self.pos += 1;
                let inner = self.parse_expr();
                if *self.peek() == TokenKind::CloseParen {
                    self.pos += 1;
                } else {
                    let span = self.current_span();
                    self.diag(span, "expected `)`", DiagSeverity::Error);
                }
                Expr::Paren(Box::new(inner))
            }
            TokenKind::StringLit(s) => {
                let span = self.current_span();
                self.pos += 1;
                Expr::StringLit(s, span)
            }
            TokenKind::Ident(s) => {
                let span = self.current_span();
                self.pos += 1;
                Expr::Symbol(s, span)
            }
            TokenKind::Macro(m) => {
                let span = self.current_span();
                self.pos += 1;
                Expr::Symbol(format!("$({})", m), span)
            }
            // Tristate literals y/n/m are identifiers in the lexer;
            // handle bare keywords that can appear in expression position.
            ref tk if is_symbol_like_keyword(tk) => {
                let name = keyword_to_str(tk).to_string();
                let span = self.current_span();
                self.pos += 1;
                Expr::Symbol(name, span)
            }
            _ => {
                let span = self.current_span();
                self.diag(span, "expected expression", DiagSeverity::Error);
                Expr::Symbol(String::new(), span)
            }
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn expect_ident(&mut self) -> (String, Span) {
        match self.peek().clone() {
            TokenKind::Ident(s) => {
                let span = self.current_span();
                self.pos += 1;
                (s, span)
            }
            ref tk if is_symbol_like_keyword(tk) => {
                let name = keyword_to_str(tk).to_string();
                let span = self.current_span();
                self.pos += 1;
                (name, span)
            }
            _ => {
                let span = self.current_span();
                self.diag(span, "expected identifier", DiagSeverity::Error);
                (String::new(), span)
            }
        }
    }

    fn expect_string(&mut self) -> (String, Span) {
        match self.peek().clone() {
            TokenKind::StringLit(s) => {
                let span = self.current_span();
                self.pos += 1;
                (s, span)
            }
            TokenKind::Ident(s) => {
                let span = self.current_span();
                self.pos += 1;
                (s, span)
            }
            TokenKind::Macro(m) => {
                let span = self.current_span();
                self.pos += 1;
                (format!("$({})", m), span)
            }
            _ => {
                let span = self.current_span();
                self.diag(span, "expected string", DiagSeverity::Error);
                (String::new(), span)
            }
        }
    }

    fn consume_type_kind(&mut self) -> TypeKind {
        let kind = match self.peek() {
            TokenKind::Bool => TypeKind::Bool,
            TokenKind::Tristate => TypeKind::Tristate,
            TokenKind::StringType => TypeKind::String,
            TokenKind::Hex => TypeKind::Hex,
            TokenKind::Int => TypeKind::Int,
            _ => TypeKind::Bool,
        };
        self.pos += 1;
        kind
    }

    fn try_parse_inline_prompt(&mut self) -> Option<PromptAttr> {
        match self.peek() {
            TokenKind::StringLit(_) => {
                let start = self.current_span();
                Some(self.parse_prompt_value(start))
            }
            _ => None,
        }
    }

    fn parse_prompt_value(&mut self, start: Span) -> PromptAttr {
        let (text, text_span) = self.expect_string();
        let condition = self.try_parse_if_condition();
        let span = start.merge(condition.as_ref().map(|e| e.span()).unwrap_or(text_span));
        PromptAttr {
            text,
            text_span,
            condition,
            span,
        }
    }

    fn try_parse_if_condition(&mut self) -> Option<Expr> {
        if *self.peek() == TokenKind::If {
            self.pos += 1;
            Some(self.parse_expr())
        } else {
            None
        }
    }
}

fn is_symbol_like_keyword(tk: &TokenKind) -> bool {
    matches!(
        tk,
        TokenKind::On
            | TokenKind::Modules
            | TokenKind::Optional
            | TokenKind::Transitional
            | TokenKind::Bool
            | TokenKind::Tristate
            | TokenKind::Hex
            | TokenKind::Int
    )
}

fn keyword_to_str(tk: &TokenKind) -> &'static str {
    match tk {
        TokenKind::On => "on",
        TokenKind::Modules => "modules",
        TokenKind::Optional => "optional",
        TokenKind::Transitional => "transitional",
        TokenKind::Bool => "bool",
        TokenKind::Tristate => "tristate",
        TokenKind::Hex => "hex",
        TokenKind::Int => "int",
        _ => "",
    }
}

fn attr_span(a: &Attribute) -> Span {
    match a {
        Attribute::Type(t) => t.span,
        Attribute::Prompt(p) => p.span,
        Attribute::Default(d) => d.span,
        Attribute::DefType(d) => d.span,
        Attribute::DependsOn(d) => d.span,
        Attribute::Select(s) => s.span,
        Attribute::Imply(i) => i.span,
        Attribute::VisibleIf(v) => v.span,
        Attribute::Range(r) => r.span,
        Attribute::Help(h) => h.span,
        Attribute::Modules(s) | Attribute::Transitional(s) | Attribute::Optional(s) => *s,
    }
}
