
use super::Editor;

use super::syntax::{Syntax, TokenType, QUOTES, SEPARATORS};
use std::mem;

#[derive(Default, Debug, PartialEq, PartialOrd, Eq, Ord)]
/// Lexer and Token
pub struct Token {
    ty: TokenType,
    buffer: String,
}

impl Token {
    pub fn new<S: Into<String>>(ty: TokenType, buffer: S) -> Self {
        Token {
            ty,
            buffer: buffer.into(),
        }
    }
    pub fn ty(&self) -> TokenType {
        self.ty
    }
    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    fn first(&mut self, c: char, syntax: &Syntax) -> Option<Self> {
        self.buffer.push(c);
        let mut token = None;
    
        self.ty = match c {
            c if c.is_whitespace() => {
                self.ty = TokenType::Whitespace(c);
                token = self.drain(self.ty);
                TokenType::Whitespace(c)
            }
            '"' => {
                if self.ty == TokenType::Str('"') {
                    // Closing quote
                    token = self.drain(TokenType::Str('"'));
                    TokenType::Unknown
                } else {
                    // Starting quote
                    TokenType::Str('"')
                }
            }
            '$' => {
                token = self.drain(TokenType::Symbol);
                TokenType::Symbol
            }
            c if syntax.is_keyword(c.to_string().as_str()) => TokenType::Keyword,
            c if syntax.is_type(c.to_string().as_str()) => TokenType::Type,
            c if syntax.is_special(c.to_string().as_str()) => TokenType::Special,
            c if syntax.comment == c.to_string().as_str() => TokenType::Comment(false),
            c if syntax.comment_multiline[0] == c.to_string().as_str() => TokenType::Comment(true),
            _ => TokenType::from(c),
        };
    
        token
    }
    
    fn drain(&mut self, ty: TokenType) -> Option<Self> {
        let mut token = None;
        if !self.buffer().is_empty() {
            token = Some(Token {
                buffer: mem::take(&mut self.buffer),
                ty: self.ty,
            });
        }
        self.ty = ty;
        token
    }

    fn push_drain(&mut self, c: char, ty: TokenType) -> Option<Self> {
        self.buffer.push(c);
        self.drain(ty)
    }

    fn drain_push(&mut self, c: char, ty: TokenType) -> Option<Self> {
        let token = self.drain(self.ty);
        self.buffer.push(c);
        self.ty = ty;
        token
    }

    /// Syntax highlighting
    pub fn highlight<T: Editor>(&mut self, editor: &T, text: &str) -> LayoutJob {
        *self = Token::default();
        let mut job = LayoutJob::default();
        for c in text.chars() {
            for token in self.automata(c, editor.syntax()) {
                editor.append(&mut job, &token);
            }
        }
        editor.append(&mut job, self);
        job
    }

    /// Lexer
    pub fn tokens(&mut self, syntax: &Syntax, text: &str) -> Vec<Self> {
        let mut tokens: Vec<Self> = text
            .chars()
            .flat_map(|c| self.automata(c, syntax))
            .collect();

        if !self.buffer.is_empty() {
            tokens.push(mem::take(self));
        }
        tokens
    }
    
    fn automata(&mut self, c: char, syntax: &Syntax) -> Vec<Self> {
        use TokenType as Ty;
        let mut tokens = vec![];
    
        match (self.ty, Ty::from(c)) {
            (Ty::Comment(false), Ty::Whitespace('\n')) => {
                self.buffer.push(c);
                let n = self.buffer.pop();
                tokens.extend(self.drain(Ty::Whitespace(c)));
                if let Some(n) = n {
                    tokens.extend(self.push_drain(n, self.ty));
                }
            }
            (Ty::Comment(false), _) => {
                self.buffer.push(c);
            }
            (Ty::Comment(true), _) => {
                self.buffer.push(c);
                if self.buffer.ends_with(syntax.comment_multiline[1]) {
                    tokens.extend(self.drain(Ty::Unknown));
                }
            }
            (Ty::Literal | Ty::Punctuation(_), Ty::Whitespace(_)) => {
                tokens.extend(self.drain(Ty::Whitespace(c)));
                tokens.extend(self.first(c, syntax));
            }
            (Ty::Hyperlink, Ty::Whitespace(_)) => {
                tokens.extend(self.drain(Ty::Whitespace(c)));
                tokens.extend(self.first(c, syntax));
            }
            (Ty::Hyperlink, _) => {
                self.buffer.push(c);
            }
            (Ty::Literal, _) => match c {
                '(' => {
                    self.ty = Ty::Function;
                    tokens.extend(self.drain(Ty::Punctuation(c)));
                    tokens.extend(self.push_drain(c, Ty::Unknown));
                }
                c if !c.is_alphanumeric() && !SEPARATORS.contains(&c) => {
                    tokens.extend(self.drain(self.ty));
                    self.buffer.push(c);
                    self.ty = if QUOTES.contains(&c) {
                        Ty::Str(c)
                    } else {
                        Ty::Punctuation(c)
                    };
                }
                _ => {
                    self.buffer.push(c);
                    self.ty = if self.buffer.starts_with(syntax.comment) {
                        Ty::Comment(false)
                    } else if self.buffer.starts_with(syntax.comment_multiline[0]) {
                        Ty::Comment(true)
                    } else if syntax.is_hyperlink(&self.buffer) {
                        Ty::Hyperlink
                    } else if syntax.is_keyword(&self.buffer) {
                        Ty::Keyword
                    } else if syntax.is_type(&self.buffer) {
                        Ty::Type
                    } else if syntax.is_special(&self.buffer) {
                        Ty::Special
                    } else {
                        Ty::Literal
                    };
                }
            },
            (Ty::Numeric(false), Ty::Punctuation('.')) => {
                self.buffer.push(c);
                self.ty = Ty::Numeric(true);
            }
            (Ty::Numeric(_), Ty::Numeric(_)) => {
                self.buffer.push(c);
            }
            (Ty::Numeric(_), Ty::Literal) => {
                tokens.extend(self.drain(self.ty));
                self.buffer.push(c);
            }
            (Ty::Numeric(_), _) | (Ty::Punctuation(_), Ty::Literal | Ty::Numeric(_)) => {
                tokens.extend(self.drain(self.ty));
                tokens.extend(self.first(c, syntax));
            }
            (Ty::Punctuation(_), Ty::Str(_)) => {
                tokens.extend(self.drain_push(c, Ty::Str(c)));
            }
            (Ty::Punctuation(_), _) => {
                if !(syntax.comment.starts_with(&self.buffer)
                    || syntax.comment_multiline[0].starts_with(&self.buffer))
                {
                    tokens.extend(self.drain(self.ty));
                    tokens.extend(self.first(c, syntax));
                } else {
                    self.buffer.push(c);
                    if self.buffer.starts_with(syntax.comment) {
                        self.ty = Ty::Comment(false);
                    } else if self.buffer.starts_with(syntax.comment_multiline[0]) {
                        self.ty = Ty::Comment(true);
                    } else if let Some(c) = self.buffer.pop() {
                        tokens.extend(self.drain(Ty::Punctuation(c)));
                        tokens.extend(self.first(c, syntax));
                    }
                }
            }
            // Handle strings
            (Ty::Str(q), _) => {
                if self.buffer.ends_with('`') {
                    // Handle escape sequences
                    self.buffer.push(c); // Preserve the escaped character
                } else if c == q {
                    // Handle closing quote
                    self.buffer.push(c);
                    tokens.extend(self.drain(Ty::Str(q))); // Emit the string token
                } else if c == '$' {
                    // Start embedded syntax
                    tokens.extend(self.drain(Ty::Str(q))); // Emit current string segment
                    tokens.push(Token {
                        ty: Ty::Symbol,                   // Emit `$` as a symbol
                        buffer: "$".to_string(),
                    });
                    self.ty = Ty::Embedded;               // Switch state to Embedded
                } else {
                    // Regular character inside the string
                    self.buffer.push(c);
                }
            }
            // Handle embedded syntax within strings
            (Ty::Embedded, _) if c.is_alphanumeric() || c == '_' || c == '.' => {
                self.buffer.push(c);
                self.ty = Ty::Variable; // Switch to Variable for identifiers
            }
            (Ty::Embedded, _) if c == '(' => {
                self.buffer.push(c);
                tokens.extend(self.drain(Ty::Punctuation('('))); // Start of embedded expression
            }
            (Ty::Embedded, _) => {
                tokens.extend(self.drain(Ty::Embedded)); // End embedded syntax
                tokens.extend(self.first(c, syntax));   // Process the next character
            }
            // Handle variables starting with `$`
            (Ty::Variable, _) if !c.is_alphanumeric() && c != '_' && c != '.' => {
                tokens.extend(self.drain(Ty::Variable)); // Emit variable token
                tokens.extend(self.first(c, syntax));   // Process the next token
            }
            (Ty::Whitespace(_) | Ty::Unknown, _) => {
                tokens.extend(self.first(c, syntax));
            }
            (Ty::Special, _) if self.buffer.ends_with("@{") => {
                tokens.extend(self.drain(Ty::Special));
            }
            // Keyword, Type, Special
            (_reserved, Ty::Literal | Ty::Numeric(_)) => {
                self.buffer.push(c);
                self.ty = if syntax.is_keyword(&self.buffer) {
                    Ty::Keyword
                } else if syntax.is_type(&self.buffer) {
                    Ty::Type
                } else if syntax.is_special(&self.buffer) {
                    Ty::Special
                } else {
                    Ty::Literal
                };
            }
            (Ty::Symbol, _) if c.is_alphanumeric() || c == '_' => {
                tokens.extend(self.drain(Ty::Variable)); // Emit the variable name after `$`
                self.buffer.push(c);
            }
            (reserved, _) => {
                self.ty = reserved;
                tokens.extend(self.drain(self.ty));
                tokens.extend(self.first(c, syntax));
            }
        }
        tokens
    }
    
}


use eframe::egui::text::LayoutJob;


impl<T: Editor> eframe::egui::util::cache::ComputerMut<(&T, &str), LayoutJob> for Token {
    fn compute(&mut self, (cache, text): (&T, &str)) -> LayoutJob {
        self.highlight(cache, text)
    }
}


pub type HighlightCache = eframe::egui::util::cache::FrameCache<LayoutJob, Token>;


pub fn highlight<T: Editor>(ctx: &eframe::egui::Context, cache: &T, text: &str) -> LayoutJob {
    ctx.memory_mut(|mem| mem.caches.cache::<HighlightCache>().get((cache, text)))
}