use std::{fmt::Display, io::Write, ops::Range};

use bstr::{BStr, ByteSlice};
use rust_decimal::Decimal;

use crate::read::NestedString;

use super::{
    lexer::{CmpOp, LogiOp, MatchOp},
    parser::{Filter, Node, Value},
};

pub fn in_place_str<const N: usize>(array: &mut [u8; N], it: impl Display) -> &str {
    let mut slice = &mut array[..];
    write!(slice, "{}", it).unwrap();
    let remaining = slice.len();
    let len = array.len() - remaining;
    std::str::from_utf8(&array[..len]).unwrap()
}

pub struct Engine<'a> {
    filter: &'a Filter,
}

impl<'r> Engine<'r> {
    pub fn new(filter: &'r Filter) -> Self {
        Self { filter }
    }

    fn by_id<'a>(&self, record: &'a NestedString, i: u32) -> &'a BStr {
        let (idx, (start, end)) = self.filter.idx[i as usize];
        let field = record.get(idx as usize).unwrap_or_default();
        BStr::new(
            &field[start.min(field.len() as u32) as usize..end.min(field.len() as u32) as usize],
        )
    }

    fn cmp<T: Eq + Ord>(a: &T, b: &T, op: CmpOp) -> bool {
        match op {
            CmpOp::Eq => a == b,
            CmpOp::Ne => a != b,
            CmpOp::Gt => a > b,
            CmpOp::Lt => a < b,
            CmpOp::Ge => a >= b,
            CmpOp::Le => a <= b,
        }
    }

    fn check_action(&self, str: &BStr, op: CmpOp, value: &Value) -> bool {
        match value {
            Value::Nb(nb) => {
                if let Some(field) = str.to_str().ok().and_then(|s| s.parse::<Decimal>().ok()) {
                    Self::cmp(&field, nb, op)
                } else {
                    let mut buff = [0; 32];
                    let nb = in_place_str(&mut buff, nb);
                    Self::cmp(&str, &BStr::new(nb), op)
                }
            }
            Value::Str(value) => Self::cmp(
                &str.as_ref(),
                &self.filter.source[value.clone()]
                    .as_bytes()
                    .trim_with(|c| c == '"'),
                op,
            ),
        }
    }

    fn compare(
        &self,
        record: &NestedString,
        id_i: u32,
        op: CmpOp,
        m: MatchOp,
        range: Range<u32>,
    ) -> bool {
        let str = self.by_id(record, id_i);
        let mut values = self.filter.values[range.start as usize..range.end as usize].into_iter();
        match m {
            MatchOp::All => values.all(|value| Self::check_action(self, str, op, value)),
            MatchOp::Any => values.any(|value| Self::check_action(self, str, op, value)),
        }
    }

    fn per_match(&self, record: &NestedString, id_i: u32, m: MatchOp, range: Range<u32>) -> bool {
        let str = self.by_id(record, id_i);
        let mut regs = self.filter.regex[range.start as usize..range.end as usize].into_iter();
        match m {
            MatchOp::All => regs.all(|value| value.is_match(str)),
            MatchOp::Any => regs.any(|value| value.is_match(str)),
        }
    }

    fn run_node(&self, record: &NestedString, i: u32) -> bool {
        match &self.filter.nodes[i as usize] {
            Node::Exist(i) => !self.by_id(record, *i).is_empty(),
            Node::Cmp { id, op, m, range } => self.compare(record, *id, *op, *m, range.clone()),
            Node::Match { id, m, range } => self.per_match(record, *id, *m, range.clone()),
            Node::Unary(inverse, id) => {
                let result = self.run_node(record, *id);
                if *inverse {
                    !result
                } else {
                    result
                }
            }
            Node::Binary { lhs, op, rhs } => {
                let (lhs, rhs) = (self.run_node(record, *lhs), self.run_node(record, *rhs));
                match op {
                    LogiOp::And => lhs && rhs,
                    LogiOp::Or => lhs || rhs,
                }
            }
        }
    }

    pub fn check(&self, record: &NestedString) -> bool {
        self.run_node(record, self.filter.start)
    }
}
