use std::ops::Range;

use reedline::LineBuffer;
use tui::{
    crossterm::event::KeyCode,
    none,
    unicode_width::{UnicodeWidthChar, UnicodeWidthStr},
    Canvas, Color, Line,
};

use crate::{
    filter::{Highlighter, Style},
    fmt::Fmt,
    prompt::{Prompt, PromptCmd},
    style, Nav,
};

pub struct Navigator {
    buff: LineBuffer,
    nav: Nav,
    from: (usize, usize),
}

impl Navigator {
    pub fn new(nav: Nav) -> Self {
        let buff = LineBuffer::new();
        Self {
            buff,
            from: (nav.c_row, nav.c_col),
            nav,
        }
    }

    pub fn nav(&mut self) -> &mut Nav {
        &mut self.nav
    }

    pub fn on_key(&mut self, code: KeyCode) -> Option<Nav> {
        if self.buff.is_empty() {
            let ret = match code {
                KeyCode::Left | KeyCode::Char('h') => {
                    self.nav.full_left();
                    true
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.nav.full_down();
                    true
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.nav.full_up();
                    true
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    self.nav.full_right();
                    true
                }
                KeyCode::Enter | KeyCode::Esc => true,
                _ => false,
            };

            if ret {
                return Some(self.nav.clone());
            }
        }

        match code {
            KeyCode::Char(c) => {
                self.buff.insert_char(c);
                let buff = self.buff.get_buffer();
                let (row, col) = buff.split_once(':').unwrap_or((buff, ""));
                let pos = (
                    row.parse::<usize>()
                        .map(|nb| nb.saturating_sub(1))
                        .unwrap_or(self.from.0),
                    col.parse::<usize>()
                        .map(|nb| nb.saturating_sub(1))
                        .unwrap_or(self.from.1),
                );
                self.nav.go_to(pos);
            }
            KeyCode::Left => self.buff.move_left(),
            KeyCode::Right => self.buff.move_right(),
            KeyCode::Backspace => self.buff.delete_left_grapheme(),
            KeyCode::Enter => return Some(self.nav.clone()),
            KeyCode::Esc => {
                self.nav.go_to(self.from);
                return Some(self.nav.clone());
            }
            _ => {}
        }

        None
    }

    pub fn draw_status(&self, l: &mut Line, fmt: &mut Fmt) {
        l.draw("Go to pos ", none());
        l.draw(fmt.amount(self.nav.c_row + 1), none());
        l.draw(':', style::secondary());
        l.draw(fmt.amount(self.nav.c_col + 1), none());
        l.draw(" over ", none());
        l.draw(fmt.amount(self.nav.m_row + 1), none());
        l.draw(':', style::secondary());
        l.draw(fmt.amount(self.nav.m_col + 1), none());
    }

    pub fn draw_prompt(&self, c: &mut Canvas) {
        if !self.buff.is_empty() {
            let mut l = c.btm();
            l.draw("$ ", style::secondary());
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

    pub fn on_key(&mut self, code: KeyCode) -> (&str, bool) {
        self.err = None;
        match code {
            KeyCode::Char(c) => {
                self.prompt.exec(PromptCmd::Write(c));
            }
            KeyCode::Left => self.prompt.exec(PromptCmd::Left),
            KeyCode::Right => self.prompt.exec(PromptCmd::Right),
            KeyCode::Up => self.prompt.exec(PromptCmd::Prev),
            KeyCode::Down => self.prompt.exec(PromptCmd::Next),
            KeyCode::Backspace => {
                self.prompt.exec(PromptCmd::Delete);
            }
            KeyCode::Enter => {
                let (str, _) = self.prompt.state();
                return (str, true);
            }
            _ => {}
        }
        let (str, _) = self.prompt.state();
        (str, false)
    }

    pub fn on_compile(&mut self) {
        self.prompt.exec(PromptCmd::New(true));
    }

    pub fn on_error(&mut self, err: (Range<usize>, &'static str), apply: bool) {
        if apply {
            self.prompt.exec(PromptCmd::Jump(err.0.start))
        }
        self.err.replace(err);
    }

    pub fn draw_prompt(&mut self, c: &mut Canvas) {
        let mut l = c.btm();
        l.draw("$ ", none().fg(Color::DarkGrey));
        let (str, cursor) = self.prompt.state();
        let mut highlighter = Highlighter::new(str);
        let mut pending_cursor = true;

        let mut w = l.width();
        self.offset = self.offset.min(cursor);

        let mut before = str[..cursor].chars().rev();
        let mut start = cursor;
        let after = str[cursor..].chars();
        let mut end = cursor;
        // Read left until goal
        loop {
            if start == self.offset {
                break;
            }
            if let Some(c) = before.next() {
                let c_width = c.width().unwrap_or(0);
                if c_width > 0 && w <= c_width {
                    break;
                }
                w -= c_width;
                start -= c.len_utf8();
            } else {
                break;
            }
        }
        self.offset = start;
        // Read right until eof
        for c in after {
            let c_width = c.width().unwrap_or(0);
            if c_width > 0 && w <= c_width {
                break;
            }
            w -= c_width;
            end += c.len_utf8();
        }
        // Read left until eof
        for c in before {
            let c_width = c.width().unwrap_or(0);
            if c_width > 0 && w <= c_width {
                break;
            }
            w -= c_width;
            start -= c.len_utf8();
        }

        for (i, c) in str[start..end].char_indices() {
            let i = start + i;
            if l.width() == 1 {
                break;
            }
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
        // Draw error message
        if let Some((range, msg)) = &self.err {
            let mut l = c.btm();
            l.draw("  ", none());
            if range.end >= start && range.start <= end {
                let mut range = range.clone();
                range.start = range.start.max(start);
                range.end = range.end.min(end);
                let space_left = str[start..range.start].width();
                let space_right = str[range.end..end]
                    .width()
                    .max(l.width().saturating_sub(space_left + range.len() + 1));
                if space_right > msg.width() && space_right >= space_left {
                    l.draw(
                        format_args!(
                            "{s:<0$}{s:▾<1$} {msg}",
                            range.start.saturating_sub(start),
                            range.len(),
                            s = ""
                        ),
                        none().fg(Color::Red),
                    );
                } else if space_left > msg.width() {
                    l.rdraw(
                        format_args!(
                            "{msg} {s:▾<1$}{s:<0$}",
                            l.width().saturating_sub(range.end.saturating_sub(start)),
                            range.len(),
                            s = ""
                        ),
                        none().fg(Color::Red),
                    );
                } else {
                    l.draw(format_args!("{msg}"), none().fg(Color::Red));
                }
            } else {
                if range.start > end {
                    l.rdraw(format_args!("{msg} ▸"), none().fg(Color::Red));
                } else {
                    l.draw(format_args!("◂ {msg}"), none().fg(Color::Red));
                }
            }
        }
    }

    pub fn draw_status(l: &mut Line, filter: &str) {
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
