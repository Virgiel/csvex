use reedline::LineBuffer;

struct HistoryBuffer<T, const N: usize> {
    ring: [T; N],
    head: usize,
    filled: bool,
}

impl<T: Default, const N: usize> HistoryBuffer<T, N> {
    pub fn new() -> Self {
        Self {
            ring: std::array::from_fn(|_| T::default()),
            head: 0,
            filled: false,
        }
    }

    pub fn push(&mut self, item: T) {
        self.ring[self.head] = item;
        if self.head + 1 == self.ring.len() {
            self.filled = true;
        }
        self.head = (self.head + 1) % self.ring.len();
    }

    pub fn get(&self, idx: usize) -> &T {
        assert!(idx <= self.ring.len() && self.len() > 0);
        let pos = (self.ring.len() + self.head - idx - 1) % self.ring.len();
        &self.ring[pos]
    }

    pub fn len(&self) -> usize {
        if self.filled {
            self.ring.len()
        } else {
            self.head
        }
    }
}

pub struct Prompt {
    history: HistoryBuffer<String, 5>,
    pos: Option<usize>,
    buffer: LineBuffer,
}

impl Prompt {
    pub fn new() -> Self {
        Self {
            history: HistoryBuffer::new(),
            pos: None,
            buffer: LineBuffer::new(),
        }
    }

    /// Ensure buffer contains the right data
    fn solidify(&mut self) {
        if let Some(pos) = self.pos.take() {
            self.buffer.clear();
            self.buffer.insert_str(self.history.get(pos));
        }
    }

    pub fn exec(&mut self, cmd: PromptCmd) {
        match cmd {
            PromptCmd::Write(c) => {
                self.solidify();
                self.buffer.insert_char(c);
            }
            PromptCmd::Left => {
                self.solidify();
                self.buffer.move_left();
            }
            PromptCmd::Right => {
                self.solidify();
                self.buffer.move_right()
            }
            PromptCmd::Delete => {
                self.solidify();
                self.buffer.delete_left_grapheme();
            }
            PromptCmd::Prev => match &mut self.pos {
                Some(pos) if *pos + 1 < self.history.len() => *pos += 1,
                None if self.history.len() > 0 => self.pos = Some(0),
                _ => {}
            },
            PromptCmd::Next => match &mut self.pos {
                Some(0) => self.pos = None,
                Some(pos) => *pos = pos.saturating_sub(1),
                None => {}
            },
            PromptCmd::New(keep) => {
                let (str, _) = self.state();
                self.history.push(str.into());
                self.pos = keep.then_some(0);
                self.buffer.clear();
            }
            PromptCmd::Jump(pos) => self.buffer.set_insertion_point(pos),
        }
    }

    pub fn state(&self) -> (&str, usize) {
        match self.pos {
            Some(pos) => {
                let str = self.history.get(pos);
                (str, str.len())
            }
            None => (self.buffer.get_buffer(), self.buffer.insertion_point()),
        }
    }
}

pub enum PromptCmd {
    Write(char),
    Left,
    Right,
    Prev,
    Next,
    New(bool),
    Delete,
    Jump(usize),
}
