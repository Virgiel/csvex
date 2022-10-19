#[derive(Clone, Copy)]
enum Constraint {
    Constrained,
    Full,
    Defined(usize),
}

pub enum SizeCmd {
    Constrain,
    Full,
    Less,
    More,
}

pub struct ColSize {
    size: Vec<(usize, Constraint)>,
}

impl ColSize {
    pub fn new() -> Self {
        Self { size: Vec::new() }
    }

    pub fn set_nb_cols(&mut self, nb_cols: usize) {
        if nb_cols > self.size.len() {
            self.size.resize(nb_cols, (0, Constraint::Constrained));
        }
    }

    pub fn register_size(&mut self, idx: usize, len: usize) {
        self.size[idx].0 = self.size[idx].0.max(len);
    }

    pub fn get_size(&mut self, idx: usize) -> usize {
        let (size, constraint) = self.size[idx];
        match constraint {
            Constraint::Constrained => size.min(25),
            Constraint::Full => size,
            Constraint::Defined(size) => size,
        }
    }

    pub fn reset(&mut self) {
        self.size.clear();
    }

    pub fn fit(&mut self) {
        for (s, _) in &mut self.size {
            *s = 0;
        }
    }

    pub fn cmd(&mut self, idx: usize, cmd: SizeCmd) {
        self.size[idx].1 = match cmd {
            SizeCmd::Constrain => Constraint::Constrained,
            SizeCmd::Full => Constraint::Full,
            SizeCmd::Less => Constraint::Defined(self.get_size(idx).saturating_sub(1)),
            SizeCmd::More => Constraint::Defined(self.get_size(idx).saturating_add(1)),
        };
    }
}
