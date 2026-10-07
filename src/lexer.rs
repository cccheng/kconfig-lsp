use crate::{
    ast::{Span, TypeKind},
    settings::Settings,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    // Top-level keywords
    Config,
    ConfigDefault,
    MenuConfig,
    Choice,
    EndChoice,
    CommentKw, // `comment` keyword (distinct from `#` line comments)
    Menu,
    EndMenu,
    If,
    EndIf,
    Source,
    MainMenu,

    // Type keywords
    Bool,
    Tristate,
    StringType,
    Hex,
    Int,

    // Attribute keywords
    Prompt,
    Default,
    DefType(TypeKind),
    Depends,
    On,
    Select,
    Imply,
    Visible,
    Range,
    Help,
    Modules,
    Transitional,
    Optional,

    // Operators
    Eq,         // =
    NotEq,      // !=
    Less,       // <
    Greater,    // >
    LessEq,     // <=
    GreaterEq,  // >=
    Not,        // !
    And,        // &&
    Or,         // ||
    OpenParen,  // (
    CloseParen, // )

    // Literals & identifiers
    StringLit(String), // "..." or '...'
    Ident(String),     // unquoted identifier / symbol

    // Macro invocation $(...)
    Macro(String),

    // Macro variable assignment: `name = value`, `name := value` or
    // `name += value`. The value is the rest of the line, unparsed.
    Assign,
    AssignValue(String),

    // Line comment: # ...
    LineComment(String),

    // Whitespace / structure
    Newline,
    Eof,
}

impl TokenKind {
    /// Whether this token is an attribute keyword that may appear in a
    /// `config`/`menuconfig` body.
    ///
    /// Centrally-maintained attribute classification. It must be kept in sync
    /// with `parse_config_attributes` (which dispatches these) by hand: adding a
    /// keyword there means adding it here too. `parse_configdefault_attributes`
    /// then rejects every member except `Default`. The `debug_assert` in
    /// `parse_config_attributes` only catches a keyword listed here without a
    /// parse arm, not the reverse (an arm added but not classified here).
    pub fn is_config_attribute(&self) -> bool {
        matches!(
            self,
            TokenKind::Bool
                | TokenKind::Tristate
                | TokenKind::StringType
                | TokenKind::Hex
                | TokenKind::Int
                | TokenKind::Prompt
                | TokenKind::Default
                | TokenKind::DefType(_)
                | TokenKind::Depends
                | TokenKind::Select
                | TokenKind::Imply
                | TokenKind::Visible
                | TokenKind::Range
                | TokenKind::Help
                | TokenKind::Modules
                | TokenKind::Transitional
                | TokenKind::Optional
        )
    }
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

// ---------------------------------------------------------------------------

pub struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    settings: &'a Settings,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str, settings: &'a Settings) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            pos: 0,
            settings,
        }
    }

    pub fn tokenize(mut self) -> Vec<Token> {
        let mut tokens: Vec<Token> = Vec::new();
        let mut line_start = 0;
        loop {
            let mut tok = self.next_token();
            match tok.kind {
                TokenKind::Eof => {
                    tokens.push(tok);
                    break;
                }
                TokenKind::Newline => line_start = tokens.len() + 1,
                TokenKind::Eq | TokenKind::Assign if is_variable_name(&tokens[line_start..]) => {
                    tok.kind = TokenKind::Assign;
                    tokens.push(tok);
                    tokens.extend(self.lex_assign_value());
                    continue;
                }
                _ => {}
            }
            tokens.push(tok);
        }
        tokens
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek2(&self) -> Option<u8> {
        self.bytes.get(self.pos + 1).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let b = self.bytes.get(self.pos).copied();
        if b.is_some() {
            self.pos += 1;
        }
        b
    }

    fn skip_spaces(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    /// Skip a `\` immediately followed by `\n` (line continuation).
    fn skip_line_continuation(&mut self) -> bool {
        if self.peek() == Some(b'\\') && self.peek2() == Some(b'\n') {
            self.pos += 2;
            true
        } else {
            false
        }
    }

    fn next_token(&mut self) -> Token {
        // Skip horizontal whitespace and line continuations.
        loop {
            self.skip_spaces();
            if !self.skip_line_continuation() {
                break;
            }
        }

        let start = self.pos;

        let Some(ch) = self.advance() else {
            return Token {
                kind: TokenKind::Eof,
                span: Span::new(start, start),
            };
        };

        match ch {
            b'\n' => Token {
                kind: TokenKind::Newline,
                span: Span::new(start, self.pos),
            },

            b'#' => {
                let text_start = self.pos;
                while let Some(b) = self.peek() {
                    if b == b'\n' {
                        break;
                    }
                    self.pos += 1;
                }
                Token {
                    kind: TokenKind::LineComment(self.src[text_start..self.pos].to_string()),
                    span: Span::new(start, self.pos),
                }
            }

            b'"' | b'\'' => self.lex_string(start, ch),

            b'$' if self.peek() == Some(b'(') => self.lex_macro(start),

            b'(' => Token {
                kind: TokenKind::OpenParen,
                span: Span::new(start, self.pos),
            },
            b')' => Token {
                kind: TokenKind::CloseParen,
                span: Span::new(start, self.pos),
            },

            b'!' if self.peek() == Some(b'=') => {
                self.pos += 1;
                Token {
                    kind: TokenKind::NotEq,
                    span: Span::new(start, self.pos),
                }
            }
            b'!' => Token {
                kind: TokenKind::Not,
                span: Span::new(start, self.pos),
            },

            b'=' => Token {
                kind: TokenKind::Eq,
                span: Span::new(start, self.pos),
            },
            b':' | b'+' if self.peek() == Some(b'=') => {
                self.pos += 1;
                Token {
                    kind: TokenKind::Assign,
                    span: Span::new(start, self.pos),
                }
            }

            b'<' if self.peek() == Some(b'=') => {
                self.pos += 1;
                Token {
                    kind: TokenKind::LessEq,
                    span: Span::new(start, self.pos),
                }
            }
            b'<' => Token {
                kind: TokenKind::Less,
                span: Span::new(start, self.pos),
            },

            b'>' if self.peek() == Some(b'=') => {
                self.pos += 1;
                Token {
                    kind: TokenKind::GreaterEq,
                    span: Span::new(start, self.pos),
                }
            }
            b'>' => Token {
                kind: TokenKind::Greater,
                span: Span::new(start, self.pos),
            },

            b'&' if self.peek() == Some(b'&') => {
                self.pos += 1;
                Token {
                    kind: TokenKind::And,
                    span: Span::new(start, self.pos),
                }
            }

            b'|' if self.peek() == Some(b'|') => {
                self.pos += 1;
                Token {
                    kind: TokenKind::Or,
                    span: Span::new(start, self.pos),
                }
            }

            _ if is_ident_start(ch) => self.lex_ident(start),

            // Skip any unexpected byte gracefully (error recovery).
            _ => self.next_token(),
        }
    }

    fn lex_string(&mut self, start: usize, quote: u8) -> Token {
        // Collect bytes, so a character of more than one byte stays whole.
        let mut value = Vec::new();
        loop {
            match self.advance() {
                Some(b) if b == quote => break,
                Some(b'\\') => {
                    if let Some(esc) = self.advance() {
                        value.push(esc);
                    }
                }
                Some(b'\n') | None => break, // unterminated string
                Some(b) => value.push(b),
            }
        }
        Token {
            kind: TokenKind::StringLit(String::from_utf8_lossy(&value).into_owned()),
            span: Span::new(start, self.pos),
        }
    }

    fn lex_macro(&mut self, start: usize) -> Token {
        // skip '('
        self.pos += 1;
        let mut depth = 1u32;
        let body_start = self.pos;
        while depth > 0 {
            // Without its `)`, a macro ends at the end of the line.
            match self.peek() {
                Some(b'\n') | None => break,
                Some(b'(') => depth += 1,
                Some(b')') => depth -= 1,
                _ => {}
            }
            self.pos += 1;
        }
        let body_end = if depth == 0 { self.pos - 1 } else { self.pos };
        Token {
            kind: TokenKind::Macro(self.src[body_start..body_end].to_string()),
            span: Span::new(start, self.pos),
        }
    }

    /// The value is taken as is up to the end of the line, so quotes,
    /// parentheses and `#` in it do not start a token.
    fn lex_assign_value(&mut self) -> Option<Token> {
        self.skip_spaces();
        let start = self.pos;
        let len = self.src[start..]
            .find('\n')
            .unwrap_or(self.src.len() - start);
        let value = self.src[start..start + len].trim_end_matches('\r');
        self.pos = start + len;
        (!value.is_empty()).then(|| Token {
            kind: TokenKind::AssignValue(value.to_string()),
            span: Span::new(start, start + value.len()),
        })
    }

    fn lex_ident(&mut self, start: usize) -> Token {
        while let Some(b) = self.peek() {
            if is_ident_cont(b) {
                self.pos += 1;
            } else {
                break;
            }
        }
        let text = &self.src[start..self.pos];
        let kind = self
            .keyword(text)
            .unwrap_or_else(|| TokenKind::Ident(text.to_string()));
        Token {
            kind,
            span: Span::new(start, self.pos),
        }
    }

    fn keyword(&self, s: &str) -> Option<TokenKind> {
        Some(match s {
            "config" => TokenKind::Config,
            "configdefault" if self.settings.zephyr_extensions => TokenKind::ConfigDefault,
            "menuconfig" => TokenKind::MenuConfig,
            "choice" => TokenKind::Choice,
            "endchoice" => TokenKind::EndChoice,
            "comment" => TokenKind::CommentKw,
            "menu" => TokenKind::Menu,
            "endmenu" => TokenKind::EndMenu,
            "if" => TokenKind::If,
            "endif" => TokenKind::EndIf,
            "source" => TokenKind::Source,
            // Zephyr: relative, optional, and both. `gsource` and `grsource`
            // are old names of `osource` and `orsource`.
            "rsource" | "osource" | "orsource" | "gsource" | "grsource"
                if self.settings.zephyr_extensions =>
            {
                TokenKind::Source
            }
            "mainmenu" => TokenKind::MainMenu,
            "bool" => TokenKind::Bool,
            "tristate" => TokenKind::Tristate,
            "string" => TokenKind::StringType,
            "hex" => TokenKind::Hex,
            "int" => TokenKind::Int,
            "prompt" => TokenKind::Prompt,
            "default" => TokenKind::Default,
            "def_bool" => TokenKind::DefType(TypeKind::Bool),
            "def_tristate" => TokenKind::DefType(TypeKind::Tristate),
            "def_int" if self.settings.zephyr_extensions => TokenKind::DefType(TypeKind::Int),
            "def_hex" if self.settings.zephyr_extensions => TokenKind::DefType(TypeKind::Hex),
            "def_string" if self.settings.zephyr_extensions => TokenKind::DefType(TypeKind::String),
            "depends" => TokenKind::Depends,
            "on" => TokenKind::On,
            "select" => TokenKind::Select,
            "imply" => TokenKind::Imply,
            "visible" => TokenKind::Visible,
            "range" => TokenKind::Range,
            "help" => TokenKind::Help,
            "---help---" => TokenKind::Help,
            "modules" => TokenKind::Modules,
            "transitional" => TokenKind::Transitional,
            "optional" => TokenKind::Optional,
            _ => return None,
        })
    }
}

/// Whether `tokens` make one word, such as `cc-option` or `$(X)$(Y)`, that
/// can name a macro variable.
pub(crate) fn is_variable_name(tokens: &[Token]) -> bool {
    !tokens.is_empty()
        && tokens
            .iter()
            .all(|t| matches!(t.kind, TokenKind::Ident(_) | TokenKind::Macro(_)))
        && tokens.windows(2).all(|w| w[0].span.end == w[1].span.start)
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_ident_cont(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_config_attribute_covers_attributes_not_entries() {
        // Attribute keywords.
        assert!(TokenKind::Default.is_config_attribute());
        assert!(TokenKind::Depends.is_config_attribute());
        assert!(TokenKind::DefType(TypeKind::Int).is_config_attribute());
        assert!(TokenKind::Help.is_config_attribute());
        // Entry keywords and structural tokens are not attributes.
        assert!(!TokenKind::Config.is_config_attribute());
        assert!(!TokenKind::MenuConfig.is_config_attribute());
        assert!(!TokenKind::If.is_config_attribute());
        assert!(!TokenKind::Eof.is_config_attribute());
        assert!(!TokenKind::Ident("x".into()).is_config_attribute());
    }
}
