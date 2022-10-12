use std::ops::Range;

use regex::bytes::Regex;
use rust_decimal::Decimal;

use super::lexer::{CmpOp, Lexer, LogiOp, MatchOp, TokenKind};

type Result<T> = std::result::Result<T, (Range<usize>, &'static str)>;

pub enum Node {
    // Action
    Exist(u32),
    Cmp {
        id: u32,
        op: CmpOp,
        m: MatchOp,
        range: Range<u32>,
    },
    Match {
        id: u32,
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

pub enum Id {
    Idx(u32),
    Name(Range<usize>),
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
            if let Some(token) = lexer.take_kind(TokenKind::SepRange) {
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
        if [TokenKind::Nb, TokenKind::Str, TokenKind::Id].contains(&token.kind) {
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
            lexer.next();
            lexer.take_kind(TokenKind::OpenExpr);
            self.parse_expr(lexer);
            lexer.take_kind(TokenKind::CloseExpr);
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

pub struct Filter {
    pub(crate) values: Vec<Value>,
    pub(crate) regex: Vec<Regex>,
    pub(crate) idx: Vec<(Id, (u32, u32))>,
    pub(crate) nodes: Vec<Node>,
    pub(crate) source: String,
    pub(crate) start: u32,
}

impl Filter {
    pub fn empty() -> Self {
        Self::compile("").unwrap()
    }

    pub fn compile(source: &str) -> Result<Self> {
        let source = source.trim();
        let mut tmp = Self {
            values: vec![],
            regex: vec![],
            idx: vec![],
            nodes: vec![],
            source: String::new(),
            start: 0,
        };
        let mut lexer = Lexer::load(source);
        if lexer.peek().kind != TokenKind::Eof {
            let start = tmp.parse_expr(&mut lexer)?;
            tmp.start = start;
        }

        tmp.source = source.to_string();
        Ok(tmp)
    }

    fn add<T>(vec: &mut Vec<T>, value: T) -> u32 {
        vec.push(value);
        (vec.len() - 1) as u32
    }

    fn parse_range(&mut self, lexer: &mut Lexer) -> Result<(u32, u32)> {
        if lexer.take_kind(TokenKind::OpenRange).is_some() {
            let start = lexer
                .take_kind(TokenKind::Nb)
                .map(|t| (t.span, t.str.parse::<u32>().ok()));
            let sep = lexer.take_kind(TokenKind::SepRange).is_some();
            let end = lexer
                .take_kind(TokenKind::Nb)
                .map(|t| (t.span, t.str.parse::<u32>().ok()));
            let (start, end) = match (start, sep, end) {
                (Some((span, None)), _, _) => return Err((span, "Expected a start index")),
                (_, _, Some((span, None))) => return Err((span, "Expected an end index")),
                (Some((start, _)), false, Some((end, _))) => {
                    return Err((start.start..end.end, "Missing span between index range"))
                }
                (Some((_, Some(start))), false, None) => (start, start),
                (Some((_, Some(start))), true, None) => (start, u32::MAX),
                (None, true, Some((_, Some(end)))) => (0, end),
                (Some((start_span, Some(start))), true, Some((end_span, Some(end)))) => {
                    if start <= end {
                        (start, end)
                    } else {
                        return Err((start_span.start..end_span.end, "Expected start < end"));
                    }
                }
                (None, true, None) => (0, u32::MAX),
                _ => return Err((lexer.offset()..lexer.offset(), "Expected range")),
            };
            if lexer.take_kind(TokenKind::CloseRange).is_none() {
                return Err((lexer.offset()..lexer.offset(), "Expected ]"));
            }
            Ok((start, end))
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
        let is_list = if lexer.take_kind(TokenKind::OpenList).is_some() {
            true
        } else if match_op.is_some() {
            return Err((lexer.offset()..lexer.offset(), "Expected {"));
        } else {
            false
        };

        let start = Self::add(vec, parse(lexer)?);
        let mut end = start;

        while lexer.take_kind(TokenKind::SepList).is_some() {
            end = Self::add(vec, parse(lexer)?);
        }

        if is_list && lexer.take_kind(TokenKind::CloseList).is_none() {
            return Err((lexer.offset()..lexer.offset(), "Expected }"));
        }

        Ok((match_op.unwrap_or(MatchOp::All), start..end + 1))
    }

    fn parse_regex(&mut self, lexer: &mut Lexer) -> Result<(MatchOp, Range<u32>)> {
        Self::list(lexer, &mut self.regex, |lexer| {
            let token = lexer.next();
            if token.kind == TokenKind::Str || token.kind == TokenKind::Id {
                Regex::new(token.str.trim_matches('"')).map_err(|_| (token.span, "Invalid regex"))
            } else {
                Err((token.span, "Expected regex"))
            }
        })
    }

    fn parse_value(&mut self, lexer: &mut Lexer) -> Result<(MatchOp, Range<u32>)> {
        Self::list(lexer, &mut self.values, |lexer| {
            let token = lexer.next();
            match token.kind {
                TokenKind::Nb => Ok(Value::Nb(token.str.parse().unwrap())),
                TokenKind::Str | TokenKind::Id => Ok(Value::Str(token.span)),
                _ => Err((token.span, "Expected a value")),
            }
        })
    }
    fn parse_id(&mut self, lexer: &mut Lexer) -> Result<u32> {
        let token = lexer.next();
        let id = match token.kind {
            TokenKind::Nb => {
                if let Some(nb) = token.str.parse::<u32>().ok().filter(|nb| nb > &0) {
                    Id::Idx(nb)
                } else {
                    return Err((token.span, "Expected an integer > 0"));
                }
            }
            TokenKind::Str | TokenKind::Id => Id::Name(token.span),
            _ => return Err((token.span, "Expected an id")),
        };
        let range = self.parse_range(lexer)?;
        Ok(Self::add(&mut self.idx, (id, range)))
    }

    fn parse_action(&mut self, lexer: &mut Lexer) -> Result<u32> {
        let id = self.parse_id(lexer)?;
        let token = lexer.peek();
        let node = match token.kind {
            TokenKind::Matches => {
                lexer.next();
                let (m, range) = self.parse_regex(lexer)?;
                Node::Match { id, m, range }
            }
            TokenKind::Cmp(op) => {
                lexer.next();
                let (m, range) = self.parse_value(lexer)?;
                Node::Cmp { id, op, m, range }
            }
            _ => Node::Exist(id),
        };
        Ok(Self::add(&mut self.nodes, node))
    }

    fn parse_expr(&mut self, lexer: &mut Lexer) -> Result<u32> {
        if lexer.take_kind(TokenKind::Not).is_some() {
            let token = lexer.next();
            if token.kind == TokenKind::OpenExpr {
                let idx = self.parse_expr(lexer)?;
                let token = lexer.next();
                if token.kind == TokenKind::CloseExpr {
                    Ok(Self::add(&mut self.nodes, Node::Unary(true, idx)))
                } else {
                    Err((token.span, "Expected )"))
                }
            } else {
                Err((token.span, "Expected ("))
            }
        } else if lexer.take_kind(TokenKind::OpenExpr).is_some() {
            let idx = self.parse_expr(lexer)?;
            let token = lexer.next();
            if token.kind == TokenKind::CloseExpr {
                Ok(Self::add(&mut self.nodes, Node::Unary(true, idx)))
            } else {
                Err((token.span, "Expected )"))
            }
        } else {
            let lhs = self.parse_action(lexer)?;
            let token = lexer.peek();
            let node = if let TokenKind::Logi(op) = token.kind {
                lexer.next();
                let rhs = self.parse_expr(lexer)?;
                Node::Binary { lhs, op, rhs }
            } else if TokenKind::Eof == token.kind {
                Node::Unary(false, lhs)
            } else {
                return Err((token.span.clone(), "Expected && or ||"));
            };
            Ok(Self::add(&mut self.nodes, node))
        }
    }
}
