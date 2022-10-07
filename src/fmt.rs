use bstr::{BStr, ByteSlice};
use rust_decimal::Decimal;
use std::fmt::{Display, Write as is_empty};
use std::io::Write as _;
use tui::unicode_width::UnicodeWidthChar;

use crate::BStrWidth;

use self::utils::padded;

/// Buffer used by fmt functions
pub type FmtBuffer = String;

pub fn rtrim(it: impl Display, buff: &mut FmtBuffer, budget: usize) -> &str {
    buff.clear();
    write!(buff, "{it}").unwrap();
    let overflow = buff
        .char_indices()
        .into_iter()
        .scan((0, 0), |(sum, prev), (mut pos, c)| {
            std::mem::swap(prev, &mut pos);
            *sum += c.width().unwrap_or(0);
            Some((pos, *sum > budget))
        })
        .find_map(|(pos, overflow)| (overflow).then(|| pos));
    if let Some(pos) = overflow {
        buff.replace_range(pos.., "…");
    }
    buff
}

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
    Nb { nb: Decimal, lhs: usize, rhs: usize },
    Str,
}

pub fn in_place_str<const N: usize>(array: &mut [u8; N], it: impl Display) -> &str {
    let mut slice = &mut array[..];
    write!(slice, "{}", it).unwrap();
    let remaining = slice.len();
    let len = array.len() - remaining;
    std::str::from_utf8(&array[..len]).unwrap()
}

impl Ty {
    pub fn guess(s: &BStr) -> Ty {
        let s = s.trim();
        if s.is_empty() {
            Ty::Str
        } else if let Ok(str) = s.to_str() {
            if let Ok(nb) = str.parse::<Decimal>() {
                let mut array = [0u8; 31];
                let str = in_place_str(&mut array, nb);
                let lhs = str.find('.').unwrap_or(str.len()); // Everything before .
                let rhs = str.len() - lhs;
                Ty::Nb { rhs, nb, lhs }
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
            Ty::Bool(_) => self.max_lhs = self.max_lhs.max(5),
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

mod utils {
    use std::fmt;

    /// Write amount of padding around content to respect 'num' padding character in total
    pub(crate) fn padded(
        f: &mut fmt::Formatter<'_>,
        num: usize,
        lambda: impl Fn(&mut fmt::Formatter<'_>) -> fmt::Result,
    ) -> fmt::Result {
        let (pre, post) = f
            .align()
            .map(|align| match align {
                fmt::Alignment::Left => (0, num),
                fmt::Alignment::Right => (num, 0),
                fmt::Alignment::Center => (num / 2, num / 2 + (num % 2 == 0) as usize),
            })
            .unwrap_or((0, 0));
        padding(f, pre)?;
        lambda(f)?;
        padding(f, post)?;
        Ok(())
    }

    /// Write 'num' padding character
    pub(crate) fn padding(f: &mut fmt::Formatter<'_>, num: usize) -> fmt::Result {
        let fill = f.fill();
        for _ in 0..num {
            f.write_fmt(format_args!("{}", fill))?;
        }
        Ok(())
    }
}

/// Display a well formatted field
pub struct Field<'a> {
    ty: &'a Ty,
    str: &'a BStr,
    stat: &'a ColStat,
}

impl<'a> Field<'a> {
    pub fn new(ty: &'a Ty, str: &'a BStr, stat: &'a ColStat) -> Self {
        Self { ty, str, stat }
    }
}

impl Display for Field<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let field_width = match self.ty {
            Ty::Bool(bool) => {
                if !bool {
                    5
                } else {
                    4
                }
            }
            Ty::Nb { rhs, .. } => self.stat.max_lhs + rhs,
            Ty::Str => {
                if self.stat.align_decimal {
                    self.stat.max_lhs
                } else {
                    let w = self.str.width();
                    f.width().unwrap_or(w).min(w)
                }
            }
        };
        let draw_width = f.width().unwrap_or(field_width);
        padded(f, draw_width.saturating_sub(field_width), |f| {
            match self.ty {
                Ty::Bool(bool) => {
                    let pad = self
                        .stat
                        .align_decimal
                        .then_some(self.stat.max_lhs)
                        .unwrap_or(0);
                    f.write_fmt(format_args!("{bool:>0$}", pad))
                }
                Ty::Nb { nb, rhs, .. } => {
                    f.write_fmt(format_args!("{:>1$}", nb, self.stat.max_lhs + rhs))
                }
                Ty::Str => f.write_fmt(format_args!("{:>1$}", self.str, field_width)),
            }
        })
    }
}
