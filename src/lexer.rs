use crate::ast::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    // Top-level keywords
    Config,
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
    DefBool,
    DefTristate,
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

    // Line comment: # ...
    LineComment(String),

    // Whitespace / structure
    Newline,
    Eof,
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
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            pos: 0,
        }
    }

    pub fn tokenize(mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token();
            let is_eof = tok.kind == TokenKind::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
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
        let mut value = String::new();
        loop {
            match self.advance() {
                Some(b) if b == quote => break,
                Some(b'\\') => {
                    if let Some(esc) = self.advance() {
                        value.push(esc as char);
                    }
                }
                Some(b'\n') | None => break, // unterminated string
                Some(b) => value.push(b as char),
            }
        }
        Token {
            kind: TokenKind::StringLit(value),
            span: Span::new(start, self.pos),
        }
    }

    fn lex_macro(&mut self, start: usize) -> Token {
        // skip '('
        self.pos += 1;
        let mut depth = 1u32;
        let body_start = self.pos;
        while depth > 0 {
            match self.advance() {
                Some(b'(') => depth += 1,
                Some(b')') => depth -= 1,
                None => break,
                _ => {}
            }
        }
        let body_end = if depth == 0 { self.pos - 1 } else { self.pos };
        Token {
            kind: TokenKind::Macro(self.src[body_start..body_end].to_string()),
            span: Span::new(start, self.pos),
        }
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
        let kind = keyword(text).unwrap_or_else(|| TokenKind::Ident(text.to_string()));
        Token {
            kind,
            span: Span::new(start, self.pos),
        }
    }
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_ident_cont(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

fn keyword(s: &str) -> Option<TokenKind> {
    Some(match s {
        "config" => TokenKind::Config,
        "menuconfig" => TokenKind::MenuConfig,
        "choice" => TokenKind::Choice,
        "endchoice" => TokenKind::EndChoice,
        "comment" => TokenKind::CommentKw,
        "menu" => TokenKind::Menu,
        "endmenu" => TokenKind::EndMenu,
        "if" => TokenKind::If,
        "endif" => TokenKind::EndIf,
        "source" => TokenKind::Source,
        "mainmenu" => TokenKind::MainMenu,
        "bool" => TokenKind::Bool,
        "tristate" => TokenKind::Tristate,
        "string" => TokenKind::StringType,
        "hex" => TokenKind::Hex,
        "int" => TokenKind::Int,
        "prompt" => TokenKind::Prompt,
        "default" => TokenKind::Default,
        "def_bool" => TokenKind::DefBool,
        "def_tristate" => TokenKind::DefTristate,
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
