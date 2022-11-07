use bstr::BStr;

use crate::reader::NestedString;

pub enum ColsCmd {
    Hide,
    Left,
    Right,
}

pub enum SizeCmd {
    Constrain,
    Full,
    Less,
    More,
}

#[derive(Clone, Copy)]
enum Constraint {
    Constrained,
    Full,
    Defined(usize),
}

pub struct Cols {
    headers: NestedString,
    map: Vec<usize>,
    size: Vec<(usize, Constraint)>,
    nb_col: usize,
    max_col: usize,
}

impl Cols {
    pub fn new(headers: NestedString) -> Self {
        Self {
            headers,
            map: vec![],
            size: vec![],
            nb_col: 0,
            max_col: 0,
        }
    }

    pub fn set_nb_cols(&mut self, nb_col: usize) {
        if nb_col > self.size.len() {
            self.size.resize(nb_col, (0, Constraint::Constrained));
        }
        for i in self.max_col..nb_col {
            self.map.push(i);
        }
        self.nb_col = nb_col;
        self.max_col = self.max_col.max(self.nb_col);
    }

    pub fn visible_col(&self) -> usize {
        self.map.len()
    }

    pub fn nb_col(&self) -> usize {
        self.nb_col
    }

    pub fn get_col(&self, idx: usize) -> (usize, &BStr) {
        let off = self.map[idx];
        (off, self.headers.get(off).unwrap_or_else(|| BStr::new("?")))
    }

    pub fn cmd(&mut self, idx: usize, cmd: ColsCmd) {
        if self.visible_col() == 0 {
            return;
        }
        match cmd {
            ColsCmd::Hide => {
                self.map.remove(idx);
            }
            ColsCmd::Left => self.map.swap(idx, idx.saturating_sub(1)),
            ColsCmd::Right => {
                if idx < self.map.len() - 1 {
                    self.map.swap(idx, idx + 1);
                }
            }
        }
    }

    pub fn set_headers(&mut self, headers: NestedString) {
        self.headers = headers;
    }

    fn offset(&self, idx: usize) -> usize {
        self.map[idx]
    }

    /* ----- Sizing ----- */

    pub fn size(&mut self, idx: usize, len: usize) -> usize {
        let off = self.offset(idx);
        self.size[off].0 = self.size[off].0.max(len);
        self.get_size(idx)
    }

    fn get_size(&mut self, idx: usize) -> usize {
        let off = self.offset(idx);
        let (size, constraint) = self.size[off];
        match constraint {
            Constraint::Constrained => size.min(25),
            Constraint::Full => size,
            Constraint::Defined(size) => size,
        }
    }

    pub fn reset_size(&mut self) {
        self.size.clear();
    }

    pub fn fit(&mut self) {
        for (s, _) in &mut self.size {
            *s = 0;
        }
    }

    pub fn size_cmd(&mut self, idx: usize, cmd: SizeCmd) {
        if self.visible_col() == 0 {
            return;
        }
        let off = self.offset(idx);
        self.size[off].1 = match cmd {
            SizeCmd::Constrain => Constraint::Constrained,
            SizeCmd::Full => Constraint::Full,
            SizeCmd::Less => Constraint::Defined(self.get_size(idx).saturating_sub(1)),
            SizeCmd::More => Constraint::Defined(self.get_size(idx).saturating_add(1)),
        };
    }
}
