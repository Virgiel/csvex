use std::ops::Range;

use regex::bytes::Regex;
use rust_decimal::Decimal;

use super::lexer::{CmpOp, Lexer, LogiOp, MatchOp, Token, TokenKind};

type Result<T> = std::result::Result<T, (Range<usize>, &'static str)>;
pub type Col = (u32, (u32, u32));

pub enum Node {
    // Action
    Exist(Col),
    Cmp {
        col: Col,
        op: CmpOp,
        m: MatchOp,
        range: Range<u32>,
    },
    Match {
        col: Col,
        m: MatchOp,
        range: Range<u32>,
    },
    // Logical
    Unary(bool, u32),
    Binary {
        lhs: u32,
        op: LogiOp,
        rhs: u32,
    },
}

pub enum Value {
    Nb(Decimal),
    Str(Range<usize>),
}

#[derive(Debug, Clone, Copy)]
pub enum Style {
    None,
    Id,
    Nb,
    Str,
    Regex,
    Action,
    Logi,
}

pub struct Highlighter {
    styles: Vec<(usize, Style)>,
    idx: usize,
}

impl Highlighter {
    pub fn new(source: &str) -> Self {
        let mut tmp = Self {
            styles: vec![(0, Style::None)],
            idx: 0,
        };
        tmp.parse_expr(&mut Lexer::load(source));
        tmp
    }

    pub fn style(&mut self, pos: usize) -> Style {
        // Move left
        while pos < self.styles[self.idx].0 {
            self.idx -= 1;
        }

        // Move right
        while self.idx < self.styles.len() && pos >= self.styles[self.idx + 1].0 {
            self.idx += 1;
        }

        self.styles[self.idx].1
    }

    fn add(&mut self, range: Range<usize>, style: Style) {
        let last = self.styles.last_mut().unwrap();
        if last.0 == range.start {
            last.1 = style;
        } else {
            self.styles.push((range.start, style));
        }
        self.styles.push((range.end, Style::None));
    }

    fn parse_range(&mut self, lexer: &mut Lexer) {
        if let Some(token) = lexer.take_kind(TokenKind::OpenRange) {
            self.add(token.span, Style::Id);
            if let Some(token) = lexer.take_kind(TokenKind::Nb) {
                self.add(token.span, Style::Id);
            }
            if let Some(token) = lexer.take_kind(TokenKind::SepRangeLen) {
                self.add(token.span, Style::Id);
            }
            if let Some(token) = lexer.take_kind(TokenKind::Nb) {
                self.add(token.span, Style::Id);
            }
            if let Some(token) = lexer.take_kind(TokenKind::CloseRange) {
                self.add(token.span, Style::Id);
            }
        }
    }

    fn list(&mut self, lexer: &mut Lexer, parse: impl Fn(&mut Self, &mut Lexer)) {
        let token = lexer.peek();
        match token.kind {
            TokenKind::Match(op) => {
                lexer.next();
                Some(op)
            }
            _ => None,
        };
        lexer.take_kind(TokenKind::OpenList);

        parse(self, lexer);

        while lexer.take_kind(TokenKind::SepList).is_some() {
            parse(self, lexer)
        }

        lexer.take_kind(TokenKind::OpenList);
    }

    fn parse_regex(&mut self, lexer: &mut Lexer) {
        self.list(lexer, |this, lexer| {
            if let Some(t) = lexer.take_kind(TokenKind::Str) {
                this.add(t.span, Style::Regex)
            }
        });
    }

    fn parse_value(&mut self, lexer: &mut Lexer) {
        self.list(lexer, |this, lexer| {
            let t = lexer.next();
            match t.kind {
                TokenKind::Nb => this.add(t.span, Style::Nb),
                TokenKind::Str | TokenKind::Id => this.add(t.span, Style::Str),
                _ => {}
            }
        })
    }

    fn parse_action(&mut self, lexer: &mut Lexer) {
        let token = lexer.next();
        if token.kind == TokenKind::Nb {
            self.add(token.span, Style::Id)
        }
        self.parse_range(lexer);

        let token = lexer.peek();
        match token.kind {
            TokenKind::Matches => {
                self.add(token.span.clone(), Style::Action);
                lexer.next();
                self.parse_regex(lexer);
            }
            TokenKind::Cmp(_) => {
                self.add(token.span.clone(), Style::Action);
                lexer.next();
                self.parse_value(lexer);
            }
            _ => {}
        };
    }

    fn parse_expr(&mut self, lexer: &mut Lexer) {
        if lexer.take_kind(TokenKind::Not).is_some() {
            self.parse_expr(lexer);
        } else if lexer.take_kind(TokenKind::OpenExpr).is_some() {
            self.parse_expr(lexer);
            lexer.take_kind(TokenKind::CloseExpr);
        } else {
            self.parse_action(lexer);
            let token = lexer.next();
            if let TokenKind::Logi(_) = token.kind {
                self.add(token.span, Style::Logi);
            } else if token.kind == TokenKind::Eof {
                self.add(token.span, Style::None);
                return;
            }
            self.parse_expr(lexer)
        }
    }
}

struct Compiler<'a> {
    filter: Filter,
    lexer: Lexer<'a>,
    nb_col: usize,
}

impl<'a> Compiler<'a> {
    fn compile(source: &'a str, nb_col: usize) -> Result<Filter> {
        let mut compiler = Self {
            filter: Filter::empty(),
            lexer: Lexer::load(source),
            nb_col,
        };

        if compiler.lexer.peek().kind != TokenKind::Eof {
            let start = compiler.parse_expr()?;
            compiler.filter.start = start;
        }

        compiler.filter.source = source.to_string();
        Ok(compiler.filter)
    }

    fn expect(&mut self, kind: TokenKind, msg: &'static str) -> Result<Token> {
        let token = self.lexer.next();
        if token.kind != kind {
            Err((token.span, msg))
        } else {
            Ok(token)
        }
    }

    fn add<T>(vec: &mut Vec<T>, value: T) -> u32 {
        vec.push(value);
        (vec.len() - 1) as u32
    }

    fn parse_range(&mut self) -> Result<(u32, u32)> {
        let token = self.lexer.peek();
        if token.kind == TokenKind::OpenRange {
            self.lexer.next();
            let (mut start, mut sep, mut end) = (None, None, None);
            let mut token = self.lexer.peek();
            let span_start = token.span.start;
            // Parse range start
            if TokenKind::Nb == token.kind {
                start = Some(
                    token
                        .str
                        .parse::<u32>()
                        .map_err(|_| (token.span.clone(), "Expect range start"))?,
                );
                self.lexer.next();
                token = self.lexer.peek();
            }
            // Parse range separator
            match token.kind {
                TokenKind::SepRangeLen => {
                    self.lexer.next();
                    token = self.lexer.peek();
                    sep = Some(true)
                }
                TokenKind::SepRangeEnd => {
                    self.lexer.next();
                    token = self.lexer.peek();
                    sep = Some(false)
                }
                _ => {}
            };
            // Parse range end
            if TokenKind::Nb == token.kind {
                end = Some(
                    token
                        .str
                        .parse::<u32>()
                        .map_err(|_| (token.span.clone(), "Expect range end"))?,
                );
                self.lexer.next();
                token = self.lexer.peek();
            }
            let span_end = token.span.end;
            self.expect(TokenKind::CloseRange, "Expect ]")?;
            Ok(match (start, sep, end) {
                (Some(start), None, None) => (start, start + 1),
                (Some(start), Some(true), Some(len)) => (start, start + len),
                (Some(start), Some(true), None) => (start, u32::MAX),
                (None, Some(true), Some(len)) => (0, len),
                (Some(start), Some(false), Some(end)) if start <= end => (start, end),
                _ => return Err((span_start..span_end, "Invalid range")),
            })
        } else {
            Ok((0, u32::MAX))
        }
    }

    fn list<T>(
        lexer: &mut Lexer,
        vec: &mut Vec<T>,
        parse: impl Fn(&mut Lexer) -> Result<T>,
    ) -> Result<(MatchOp, Range<u32>)> {
        let token = lexer.peek();
        let match_op = match token.kind {
            TokenKind::Match(op) => {
                lexer.next();
                Some(op)
            }
            _ => None,
        };
        let token = lexer.peek();
        let is_list = if token.kind == TokenKind::OpenList {
            lexer.next();
            true
        } else if match_op.is_some() {
            return Err((token.span.clone(), "Expect {"));
        } else {
            false
        };

        let start = Self::add(vec, parse(lexer)?);
        let mut end = start;

        while lexer.take_kind(TokenKind::SepList).is_some() {
            end = Self::add(vec, parse(lexer)?);
        }
        if is_list {
            let token = lexer.next();
            if token.kind != TokenKind::CloseList {
                return Err((token.span, "Expect }"));
            }
        }

        Ok((match_op.unwrap_or(MatchOp::All), start..end + 1))
    }

    fn parse_regex(&mut self) -> Result<(MatchOp, Range<u32>)> {
        Self::list(&mut self.lexer, &mut self.filter.regex, |lexer| {
            let token = lexer.next();
            if token.kind == TokenKind::Str || token.kind == TokenKind::Id {
                Regex::new(token.str.trim_matches('"')).map_err(|_| (token.span, "Invalid regex"))
            } else {
                Err((token.span, "Expect regex"))
            }
        })
    }

    fn parse_value(&mut self) -> Result<(MatchOp, Range<u32>)> {
        Self::list(&mut self.lexer, &mut self.filter.values, |lexer| {
            let token = lexer.next();
            match token.kind {
                TokenKind::Nb => Ok(Value::Nb(token.str.parse().unwrap())),
                TokenKind::Str | TokenKind::Id => Ok(Value::Str(token.span)),
                _ => Err((token.span, "Expect a value")),
            }
        })
    }
    fn parse_col(&mut self) -> Result<Col> {
        let token = self.lexer.next();
        let id = match token.kind {
            TokenKind::Nb => {
                if let Ok(nb) = token.str.parse::<u32>() {
                    if nb as usize >= self.nb_col {
                        return Err((token.span, "No column with this index"));
                    }
                    nb
                } else {
                    return Err((token.span, "Expect a column index"));
                }
            }
            _ => return Err((token.span, "Expect a column index")),
        };
        let range = self.parse_range()?;
        Ok((id, range))
    }

    fn parse_action(&mut self) -> Result<u32> {
        let col = self.parse_col()?;
        let token = self.lexer.peek();
        let node = match token.kind {
            TokenKind::Matches => {
                self.lexer.next();
                let (m, range) = self.parse_regex()?;
                Node::Match { col, m, range }
            }
            TokenKind::Cmp(op) => {
                self.lexer.next();
                let (m, range) = self.parse_value()?;
                Node::Cmp { col, op, m, range }
            }
            _ => Node::Exist(col),
        };
        Ok(Self::add(&mut self.filter.nodes, node))
    }

    fn parse_expr(&mut self) -> Result<u32> {
        if self.lexer.take_kind(TokenKind::Not).is_some() {
            let idx = self.parse_expr()?;
            Ok(Self::add(&mut self.filter.nodes, Node::Unary(true, idx)))
        } else if self.lexer.take_kind(TokenKind::OpenExpr).is_some() {
            let idx = self.parse_expr()?;
            self.expect(TokenKind::CloseExpr, "Expect )")?;
            Ok(Self::add(&mut self.filter.nodes, Node::Unary(true, idx)))
        } else {
            let lhs = self.parse_action()?;
            let token = self.lexer.peek();
            let node = if let TokenKind::Logi(op) = token.kind {
                self.lexer.next();
                let rhs = self.parse_expr()?;
                Node::Binary { lhs, op, rhs }
            } else if TokenKind::Eof == token.kind {
                Node::Unary(false, lhs)
            } else {
                return Err((token.span.clone(), "Expect && or ||"));
            };
            Ok(Self::add(&mut self.filter.nodes, node))
        }
    }
}

pub struct Filter {
    pub(crate) values: Vec<Value>,
    pub(crate) regex: Vec<Regex>,
    pub(crate) nodes: Vec<Node>,
    pub(crate) source: String,
    pub(crate) start: u32,
}

impl Filter {
    pub fn empty() -> Self {
        Self {
            values: vec![],
            regex: vec![],
            nodes: vec![],
            source: String::new(),
            start: 0,
        }
    }

    pub fn new(source: &str, nb_col: usize) -> Result<Self> {
        Compiler::compile(source, nb_col)
    }
}
