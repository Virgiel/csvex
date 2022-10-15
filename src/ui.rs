use std::ops::Range;

use reedline::LineBuffer;
use tui::{crossterm::event::KeyCode, none, Canvas, Color, Line};

use crate::{
    filter::{Highlighter, Style},
    fmt,
    prompt::{Prompt, PromptCmd},
    Nav,
};

pub struct Navigator {
    buff: LineBuffer,
}

impl Navigator {
    pub fn new(nav: &Nav) -> Self {
        let mut buff = LineBuffer::new();
        buff.set_buffer(format!("{}:{}", nav.c_row, nav.c_col));
        Self { buff }
    }

    fn parse(&self) -> (Option<usize>, Option<usize>) {
        let buff = self.buff.get_buffer();
        let (line, col) = buff.split_once(':').unwrap_or((buff, ""));
        (line.parse().ok(), col.parse().ok())
    }

    pub fn on_key(&mut self, code: KeyCode) -> Option<(Option<usize>, Option<usize>)> {
        match code {
            KeyCode::Char(c) => self.buff.insert_char(c),
            KeyCode::Left => self.buff.move_left(),
            KeyCode::Right => self.buff.move_right(),
            KeyCode::Backspace => self.buff.delete_left_grapheme(),
            KeyCode::Enter => {
                return Some(self.parse());
            }
            _ => {}
        }

        None
    }

    pub fn draw(&self, c: &mut Canvas, nav: &Nav) {
        let mut l = c.btm();
        let (row, col) = self.parse();
        let row = row.unwrap_or(nav.c_row);
        let col = col.unwrap_or(nav.c_col);
        l.draw("Go to pos ", none());
        l.draw(fmt::quantity(row), none());
        l.draw(':', none().fg(Color::DarkGrey));
        l.draw(fmt::quantity(col), none());
        l.draw(" over ", none());
        l.draw(fmt::quantity(nav.m_row), none());
        l.draw(':', none().fg(Color::DarkGrey));
        l.draw(fmt::quantity(nav.m_col), none());
        let mut l = c.btm();
        l.draw("$ ", none().fg(Color::DarkGrey));
        let (str, cursor) = (self.buff.get_buffer(), self.buff.insertion_point());
        let mut pending_cursor = true;

        for (i, c) in str.char_indices() {
            if pending_cursor && cursor <= i {
                l.cursor();
                pending_cursor = false
            }
            l.draw(c, none());
        }
        if pending_cursor {
            l.cursor();
        }
    }
}

pub struct FilterPrompt {
    prompt: Prompt,
    offset: usize,
    err: Option<(Range<usize>, &'static str)>,
}

impl FilterPrompt {
    pub fn new() -> Self {
        Self {
            prompt: Prompt::new(),
            offset: 0,
            err: None,
        }
    }

    pub fn on_key(&mut self, code: KeyCode) -> Option<&str> {
        self.err = None;
        match code {
            KeyCode::Char(c) => self.prompt.exec(PromptCmd::Write(c)),
            KeyCode::Left => self.prompt.exec(PromptCmd::Left),
            KeyCode::Right => self.prompt.exec(PromptCmd::Right),
            KeyCode::Up => self.prompt.exec(PromptCmd::Prev),
            KeyCode::Down => self.prompt.exec(PromptCmd::Next),
            KeyCode::Backspace => self.prompt.exec(PromptCmd::Delete),
            KeyCode::Enter => {
                let (str, _) = self.prompt.state();
                return Some(str);
            }
            _ => {}
        }
        None
    }

    pub fn on_compile(&mut self) {
        self.prompt.exec(PromptCmd::New);
    }

    pub fn on_error(&mut self, err: (Range<usize>, &'static str)) {
        self.err.replace(err);
    }

    pub fn draw(&mut self, c: &mut Canvas) {
        let mut l = c.btm();
        l.draw("$ ", none().fg(Color::DarkGrey));
        let (str, cursor) = self.prompt.state();
        let mut highlighter = Highlighter::new(str);
        let mut pending_cursor = true;

        for (i, c) in str.char_indices() {
            if pending_cursor && cursor <= i {
                l.cursor();
                pending_cursor = false
            }
            l.draw(
                c,
                match highlighter.style(i) {
                    Style::None | Style::Logi => none(),
                    Style::Id => none().fg(Color::Blue),
                    Style::Nb => none().fg(Color::Yellow),
                    Style::Str => none().fg(Color::Green),
                    Style::Regex => none().fg(Color::Magenta),
                    Style::Action => none().fg(Color::Red),
                },
            );
        }
        if pending_cursor {
            l.cursor();
        }
        if let Some((range, msg)) = &self.err {
            c.btm().draw(
                format_args!(
                    "{s:<0$}{s:▾<1$} {msg}",
                    range.start + 2,
                    range.len(),
                    s = ""
                ),
                none().fg(Color::Red),
            );
        }
    }

    pub fn draw_filter(l: &mut Line, filter: &str) {
        let mut highlighter = Highlighter::new(filter);
        for (i, c) in filter.char_indices() {
            if l.width() == 0 {
                return;
            }
            l.draw(
                c,
                match highlighter.style(i) {
                    Style::None | Style::Logi => none(),
                    Style::Id => none().fg(Color::Blue),
                    Style::Nb => none().fg(Color::Yellow),
                    Style::Str => none().fg(Color::Green),
                    Style::Regex => none().fg(Color::Magenta),
                    Style::Action => none().fg(Color::Red),
                },
            );
        }
    }
}
