use bstr::{BStr, ByteSlice};
use rust_decimal::Decimal;
use std::fmt::Write;
use tui::{unicode_width::UnicodeWidthChar, Line, Style};

use crate::BStrWidth;

/// Buffer used by fmt functions
pub type FmtBuffer = String;

pub fn quantity(buff: &mut FmtBuffer, nb: usize) -> &str {
    buff.clear();
    write!(buff, "{nb}").unwrap();
    let mut c = buff.len();
    while c > 3 {
        buff.insert(c, '_');
        c -= 3;
    }
    buff
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Ty {
    Bool(bool),
    Nb(Decimal),
    Str,
}

impl Ty {
    pub fn guess(s: &BStr) -> Ty {
        if s.is_empty() {
            Ty::Str
        } else if let Ok(str) = s.to_str() {
            if let Ok(nb) = str.parse() {
                Ty::Nb(nb)
            } else {
                match str {
                    "true" | "True" | "TRUE" => Ty::Bool(true),
                    "false" | "False" | "FALSE" => Ty::Bool(false),
                    _ => Ty::Str,
                }
            }
        } else {
            Ty::Str
        }
    }

    pub fn is_str(&self) -> bool {
        matches!(self, Ty::Str)
    }

    pub fn fmt(
        &self,
        l: &mut Line,
        s: &BStr,
        budget: usize,
        space: &ColSpacing,
        buff: &mut FmtBuffer,
        style: Style,
        is_header: bool,
    ) {
        let before = l.width();
        if before == 0 {
            return;
        }

        match self {
            Ty::Bool(bool) => {
                let pad = space.align_decimal.then_some(space.max_lhs).unwrap_or(0);
                l.draw(format!("{bool:>0$}", pad), style);
            }
            Ty::Nb(nb) => {
                buff.clear();
                write!(buff, "{nb}").unwrap();
                let lhs = buff.find('.').unwrap_or(buff.len());
                let pad = budget - space.max_lhs - space.max_rhs;
                l.draw(
                    format!("{:>1$}", BStr::new(&buff[..lhs]), space.max_lhs + pad),
                    style,
                );
                l.draw(format!("{}", BStr::new(&buff[lhs..])), style);
            }
            Ty::Str => {
                let pad = (space.align_decimal && !is_header)
                    .then_some(space.max_lhs)
                    .unwrap_or(0);
                let max = budget.min(l.width());
                // Find position where width exceed
                let overflow_pos = s
                    .char_indices()
                    .into_iter()
                    .scan((0, 0), |(sum, prev), (mut pos, _, c)| {
                        std::mem::swap(prev, &mut pos);
                        *sum += c.width().unwrap_or(0);
                        Some((pos, *sum > max))
                    })
                    .find_map(|(pos, overflow)| (overflow).then_some(pos));
                if let Some(pos) = overflow_pos {
                    l.draw(
                        format!("{:>1$}", BStr::new(&s[..pos]), pad.saturating_sub(1)),
                        style,
                    );
                    l.draw("…", style);
                } else {
                    l.draw(format!("{s:>0$}", pad), style);
                }
            }
        };
        let missing_padding = budget
            .saturating_sub(before.saturating_sub(l.width()))
            .min(l.width());
        l.draw(format_args!("{:>1$}", "", missing_padding), style);
    }
}

pub struct ColSpacing {
    header_len: usize,
    align_decimal: bool,
    only_str: bool,
    max_lhs: usize,
    max_rhs: usize,
}

impl ColSpacing {
    pub fn new() -> Self {
        Self {
            header_len: 0,
            align_decimal: false,
            only_str: true,
            max_lhs: 0,
            max_rhs: 0,
        }
    }

    pub fn header(&mut self, s: &BStr) {
        self.header_len = s.width();
    }

    pub fn add(&mut self, buff: &mut FmtBuffer, ty: &Ty, s: &BStr) {
        self.only_str &= ty.is_str();
        match ty {
            Ty::Bool(_) => self.max_lhs = self.max_lhs.max(5),
            Ty::Nb(nb) => {
                buff.clear();
                write!(buff, "{nb}").unwrap();
                let lhs = buff.find('.').unwrap_or(buff.len());
                let rhs = buff.len() - lhs;
                self.max_lhs = self.max_lhs.max(lhs);
                self.max_rhs = self.max_rhs.max(rhs);
                self.align_decimal = true;
            }
            Ty::Str => self.max_lhs = self.max_lhs.max(s.width()),
        }
    }

    pub fn budget(&self) -> usize {
        let desired = (self.max_lhs + self.max_rhs).max(self.header_len);
        if self.only_str {
            desired.min(25)
        } else {
            desired.min(40)
        }
    }
}
