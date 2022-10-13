use std::ops::Range;

use rust_decimal::Decimal;

/** This is a pull lexer responsible for finding tokens in a code line.
It is designed to not allocate any memory. */

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Cmp(CmpOp),
    Logi(LogiOp),
    Match(MatchOp),
    Matches,    // matches, ~,
    Not,        // not, !
    OpenExpr,   // (
    CloseExpr,  // )
    OpenRange,  // [
    CloseRange, // ]
    SepRange,   // :
    OpenList,   // {
    CloseList,  // }
    SepList,    // ,
    Nb,         // Decimal Number
    Str,        // surrounded by "
    Id,         // surrounded by whitespace
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq, // eq, ==
    Ne, // ne, !=
    Gt, // gt, >
    Lt, // lt, <
    Ge, // ge, >=
    Le, // le, <=
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogiOp {
    And, // and, &&
    Or,  // or, ||
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchOp {
    All, // all
    Any, // any
}

/// A code token
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token<'a> {
    pub kind: TokenKind,
    pub span: Range<usize>,
    pub str: &'a str,
}

#[derive(Debug, Clone)]
pub struct Lexer<'a> {
    source: &'a str,
    offset: usize,
    peeked: Option<Token<'a>>,
}

impl<'a> Lexer<'a> {
    /// Init the lexer at the beginning of a source
    pub fn load(source: &'a str) -> Lexer {
        Lexer {
            source,
            offset: 0,
            peeked: None,
        }
    }

    pub fn next(&mut self) -> Token<'a> {
        self.peeked.take().unwrap_or_else(|| self.lex_next())
    }

    pub fn take_kind(&mut self, kind: TokenKind) -> Option<Token> {
        (self.peek().kind == kind).then(|| self.next())
    }

    fn token(&mut self, kind: TokenKind, len: usize) -> Token<'a> {
        self.offset += len;
        let span = self.offset - len..self.offset;
        Token {
            kind,
            str: &self.source[span.clone()],
            span,
        }
    }

    pub fn peek(&mut self) -> &Token<'a> {
        if self.peeked.is_none() {
            self.peeked = Some(self.lex_next());
        }
        self.peeked.as_ref().unwrap()
    }

    fn lex_next(&mut self) -> Token<'a> {
        // Skip whitespace
        let until = self.source[self.offset..]
            .char_indices()
            .find_map(|(i, c)| (!c.is_whitespace()).then_some(i))
            .unwrap_or(0);
        self.offset += until;
        let remaining = &self.source[self.offset..];
        let bytes = remaining.as_bytes();

        if let Some(first) = bytes.first() {
            // Two char keyword
            if let Some(second) = bytes.get(1) {
                let kind = match [*first, *second] {
                    [b'=', b'='] => TokenKind::Cmp(CmpOp::Eq),
                    [b'!', b'='] => TokenKind::Cmp(CmpOp::Ne),
                    [b'>', b'='] => TokenKind::Cmp(CmpOp::Ge),
                    [b'<', b'='] => TokenKind::Cmp(CmpOp::Le),
                    [b'&', b'&'] => TokenKind::Logi(LogiOp::And),
                    [b'|', b'|'] => TokenKind::Logi(LogiOp::Or),
                    _ => TokenKind::Eof,
                };
                if kind != TokenKind::Eof {
                    return self.token(kind, 2);
                }
            }

            // One char keyword
            let kind = match *first {
                b'>' => TokenKind::Cmp(CmpOp::Gt),
                b'<' => TokenKind::Cmp(CmpOp::Lt),
                b'~' => TokenKind::Matches,
                b'!' => TokenKind::Not,
                b'(' => TokenKind::OpenExpr,
                b')' => TokenKind::CloseExpr,
                b'{' => TokenKind::OpenList,
                b'}' => TokenKind::CloseList,
                b'[' => TokenKind::OpenRange,
                b']' => TokenKind::CloseRange,
                b',' => TokenKind::SepList,
                b':' => TokenKind::SepRange,
                _ => TokenKind::Eof,
            };

            if kind != TokenKind::Eof {
                return self.token(kind, 1);
            }
        }

        let mut chars = remaining.char_indices();
        if let Some((_, c)) = chars.next() {
            match c {
                '"' => {
                    // Search next "
                    let len = chars
                        .find_map(|(i, c)| (c == '"').then_some(i + 1))
                        .unwrap_or(remaining.len());
                    self.token(TokenKind::Str, len)
                }
                c if c.is_ascii_digit() => {
                    let len = chars
                        .find_map(|(i, c)| (!c.is_ascii_digit()).then_some(i))
                        .unwrap_or(remaining.len());
                    self.token(TokenKind::Nb, len)
                }
                _ => {
                    // Search end of possible slice
                    let len = chars
                        .find_map(|(i, c)| (!c.is_alphanumeric()).then_some(i))
                        .unwrap_or(remaining.len());
                    let kind = match &remaining[..len] {
                        "eq" => TokenKind::Cmp(CmpOp::Eq),
                        "ne" => TokenKind::Cmp(CmpOp::Ne),
                        "gt" => TokenKind::Cmp(CmpOp::Gt),
                        "lt" => TokenKind::Cmp(CmpOp::Lt),
                        "ge" => TokenKind::Cmp(CmpOp::Ge),
                        "le" => TokenKind::Cmp(CmpOp::Le),
                        "and" => TokenKind::Logi(LogiOp::And),
                        "or" => TokenKind::Logi(LogiOp::Or),
                        "all" => TokenKind::Match(MatchOp::All),
                        "any" => TokenKind::Match(MatchOp::Any),
                        "matches" => TokenKind::Matches,
                        "not" => TokenKind::Not,
                        str => {
                            if str.parse::<Decimal>().is_ok() {
                                TokenKind::Nb
                            } else {
                                TokenKind::Id
                            }
                        }
                    };
                    self.token(kind, len)
                }
            }
        } else {
            self.token(TokenKind::Eof, 0)
        }
    }
}
