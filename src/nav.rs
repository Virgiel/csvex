#[derive(Clone)]
pub struct Nav {
    // Offset position
    pub o_row: usize,
    pub o_col: usize,
    // Cursor positions
    pub c_row: usize,
    pub c_col: usize,
    // Max
    pub m_row: usize,
    pub m_col: usize,
}

impl Nav {
    pub fn new() -> Self {
        Self {
            o_row: 0,
            o_col: 0,
            c_row: 0,
            c_col: 0,
            m_row: 0,
            m_col: 0,
        }
    }

    pub fn up(&mut self) {
        self.c_row = self.c_row.saturating_sub(1);
    }

    pub fn down(&mut self) {
        self.c_row = self.c_row.saturating_add(1);
    }

    pub fn left(&mut self) {
        self.c_col = self.c_col.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.c_col = self.c_col.saturating_add(1);
    }

    pub fn full_up(&mut self) {
        self.c_row = 0;
    }

    pub fn full_down(&mut self) {
        self.c_row = self.m_row;
    }

    pub fn full_left(&mut self) {
        self.c_col = 0;
    }

    pub fn full_right(&mut self) {
        self.c_col = self.m_col;
    }

    pub fn row_offset(&mut self, total: usize, nb: usize) -> usize {
        self.m_row = total.saturating_sub(1);
        // Ensure cursor pos fit in grid dimension
        self.c_row = self.c_row.min(self.m_row);
        // Ensure cursor is in view
        if self.c_row < self.o_row {
            self.o_row = self.c_row;
        } else if self.c_row >= self.o_row + nb {
            self.o_row = self.c_row - nb + 1;
        }
        self.o_row
    }

    pub fn col_iter(&mut self, total: usize, mut fit: impl FnMut(usize) -> bool) {
        self.m_col = total.saturating_sub(1);
        // Ensure cursor pos fit in grid dimension
        self.c_col = self.c_col.min(self.m_col);
        // Ensure cursor is in view
        if self.c_col < self.o_col {
            self.o_col = self.c_col;
        }

        let mut count = 0;
        let goal_l = self.o_col;
        self.o_col = self.c_col;
        if total > 0 {
            loop {
                let off = if goal_l + count <= self.c_col {
                    // Fill left until goal
                    self.c_col - count
                } else if goal_l + count <= self.m_col {
                    // Then fill right
                    goal_l + count
                } else if count <= self.m_col {
                    // Then fill left
                    self.m_col - count
                } else {
                    // No more columns
                    break;
                };
                count += 1;
                let is_fitting = fit(off);
                if is_fitting || off >= goal_l {
                    self.o_col = self.o_col.min(off);
                }
                if !is_fitting {
                    break;
                }
            }
        }
    }

    pub fn go_to(&mut self, (row, col): (usize, usize)) {
        self.c_row = row;
        self.c_col = col;
    }
}
