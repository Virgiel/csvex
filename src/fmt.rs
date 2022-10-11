use bstr::{BStr, ByteSlice};
use rust_decimal::Decimal;
use std::fmt::{Display, Write as is_empty};
use tui::unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::BStrWidth;

/// Buffer used by fmt functions
pub type FmtBuffer = String;

pub fn quantity(buff: &mut FmtBuffer, nb: usize) -> &str {
    buff.clear();
    write!(buff, "{nb}").unwrap();
    let mut c = buff.len();
    while c > 3 {
        c -= 3;
        buff.insert(c, '_');
    }
    buff
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Ty {
    Bool,
    Nb { lhs: usize, rhs: usize },
    Str,
}

impl Ty {
    pub fn guess(s: &BStr) -> Ty {
        if s.is_empty() {
            Ty::Str
        } else {
            if let Ok(s) = s.to_str() {
                if s.parse::<Decimal>().is_ok() {
                    let lhs = s.find('.').unwrap_or(s.len()); // Everything before .
                    let rhs = s.len() - lhs;
                    Ty::Nb { rhs, lhs }
                } else {
                    match s {
                        "true" | "True" | "TRUE" | "false" | "False" | "FALSE" => Ty::Bool,
                        _ => Ty::Str,
                    }
                }
            } else {
                Ty::Str
            }
        }
    }

    pub fn is_str(&self) -> bool {
        matches!(self, Ty::Str)
    }
}

pub struct ColStat {
    header_len: usize,
    align_decimal: bool,
    only_str: bool,
    max_lhs: usize,
    max_rhs: usize,
}

impl ColStat {
    pub fn new() -> Self {
        Self {
            header_len: 0,
            align_decimal: false,
            only_str: true,
            max_lhs: 0,
            max_rhs: 0,
        }
    }

    pub fn header_name(&mut self, s: &BStr) {
        self.header_len = s.width();
    }

    pub fn header_idx(&mut self, i: usize) {
        self.header_len = (i as f64).log10() as usize + 1;
    }

    pub fn add(&mut self, ty: &Ty, s: &BStr) {
        self.only_str &= ty.is_str();
        match ty {
            Ty::Bool => self.max_lhs = self.max_lhs.max(5),
            Ty::Nb { lhs, rhs, .. } => {
                self.max_lhs = self.max_lhs.max(*lhs);
                self.max_rhs = self.max_rhs.max(*rhs);
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

pub fn fmt_field<'a>(
    buff: &'a mut FmtBuffer,
    ty: &Ty,
    str: &BStr,
    stat: &ColStat,
    budget: usize,
) -> &'a str {
    buff.clear();
    let pad = match ty {
        Ty::Bool | Ty::Str if stat.align_decimal => {
            for _ in 0..budget.saturating_sub(stat.max_lhs + stat.max_rhs) {
                buff.write_char(' ').unwrap();
            }
            stat.max_lhs
        }
        Ty::Bool | Ty::Str => 0,
        Ty::Nb { rhs, .. } => {
            for _ in 0..budget.saturating_sub(stat.max_lhs + stat.max_rhs) {
                buff.write_char(' ').unwrap();
            }
            stat.max_lhs + rhs
        }
    };
    write!(buff, "{str:>0$}", pad).unwrap();
    for _ in 0..budget.saturating_sub(buff.width()) {
        buff.write_char(' ').unwrap();
    }
    trim_buffer(buff, budget)
}

fn trim_buffer(buff: &mut FmtBuffer, budget: usize) -> &str {
    let overflow = buff
        .char_indices()
        .into_iter()
        .scan((0, 0), |(sum, prev), (mut pos, c)| {
            std::mem::swap(prev, &mut pos);
            *sum += c.width().unwrap_or(0);
            Some((pos, *sum > budget))
        })
        .find_map(|(pos, overflow)| (overflow).then_some(pos));
    if let Some(pos) = overflow {
        buff.replace_range(pos.., "…");
    }
    buff
}

pub fn rtrim(it: impl Display, buff: &mut FmtBuffer, budget: usize) -> &str {
    buff.clear();
    write!(buff, "{it}").unwrap();
    trim_buffer(buff, budget)
}
