use std::{
    io::{self},
    ops::Add,
    time::Duration,
};

use bstr::{BStr, ByteSlice};
use filter::Filter;
use fmt::{ColStat, Fmt, Ty};
use index::Indexer;
use read::{Config, CsvReader, NestedString};
use size::{ColSize, SizeCmd};
use source::FileWatcher;
use spinner::Spinner;
use tui::{
    crossterm::event::{self, Event, KeyCode},
    unicode_width::UnicodeWidthChar,
    Canvas, Terminal,
};
use ui::{FilterPrompt, Navigator};

mod filter;
mod fmt;
mod index;
mod prompt;
mod read;
mod size;
mod source;
mod spinner;
mod style;
mod ui;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub const BUF_LEN: usize = 8 * 1024;

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "../../Downloads/adresses-france.csv".to_string());
    let mut app = App::open(path.clone()).unwrap();
    let mut watcher = FileWatcher::new(path).unwrap();
    let mut redraw = true;
    let mut terminal = Terminal::new(io::stdout()).unwrap();
    loop {
        // Check loading state before drawing to no skip completed task during drawing
        let is_loading = app.is_loading();
        if redraw {
            terminal.draw(|c| app.draw(c)).unwrap();
            redraw = false;
        }
        if event::poll(Duration::from_millis(250)).unwrap() {
            loop {
                if app.on_event(event::read().unwrap()) {
                    return;
                }
                // Ingest more event before drawing if we can
                if !event::poll(Duration::from_millis(0)).unwrap() {
                    break;
                }
            }
            redraw = true;
        }
        if watcher.has_change().unwrap() {
            app.on_file_change();
            redraw = true;
        }
        if is_loading {
            redraw = true;
        }
    }
}

#[derive(Clone)]

pub struct Nav {
    // Offset position
    o_row: usize,
    o_col: usize,
    // Cursor positions
    c_row: usize,
    c_col: usize,
    // View dimension
    v_row: usize,
    v_col: usize,
    // Max
    m_row: usize,
    m_col: usize,
}

impl Nav {
    pub fn new() -> Self {
        Self {
            o_row: 0,
            o_col: 0,
            c_row: 0,
            c_col: 0,
            v_row: 0,
            v_col: 0,
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
        self.m_row = total;
        // Sync view dimension
        self.v_row = nb;
        // Ensure cursor pos fit in grid dimension
        self.c_row = self.c_row.min(total.saturating_sub(1));
        // Ensure cursor is in view
        if self.c_row < self.o_row {
            self.o_row = self.c_row;
        } else if self.c_row >= self.o_row + nb {
            self.o_row = self.c_row - nb + 1;
        }
        self.o_row
    }

    pub fn col_iter(&mut self, total: usize) -> impl Iterator<Item = usize> + '_ {
        self.m_col = total;
        // Ensure cursor pos fit in grid dimension
        self.c_col = self.c_col.min(total.saturating_sub(1));
        // Reset view dimension
        self.v_col = 0;

        let amount_right = total - self.c_col;
        let goal_left = self.c_col.saturating_sub(self.o_col);

        // Coll offset iterator
        std::iter::from_fn(move || -> Option<usize> {
            if self.v_col < total {
                let step = self.v_col;
                self.v_col += 1;
                let result = if step <= goal_left {
                    // Reach goal
                    self.c_col - step
                } else if step < goal_left + amount_right {
                    // Then fill right
                    self.c_col + (step - goal_left)
                } else {
                    // Then fill left
                    self.c_col - (step - goal_left - amount_right)
                };
                if result < self.o_col {
                    self.o_col = result;
                } else if result > self.o_col + step {
                    self.o_col = result - step;
                }
                Some(result)
            } else {
                None
            }
        })
    }

    pub fn cursor_row(&self) -> usize {
        self.c_row - self.o_row
    }

    pub fn cursor_col(&self) -> usize {
        self.c_col
    }

    pub fn go_to(&mut self, (row, col): (usize, usize)) {
        self.c_row = row;
        self.c_col = col;
    }
}

struct App {
    config: Config,
    rdr: CsvReader,
    grid: Grid,
    nav: Nav,
    indexer: Indexer,
    spinner: Spinner,
    fmt: Fmt,
    dirty: bool,
    err: String,
    size: bool,
    // Filter prompt
    focus_filter_prompt: bool,
    filter_prompt: FilterPrompt,
    // Navigator
    navigator: Option<Navigator>,
    // Cols
    cols: ColSize,
}

impl App {
    pub fn open(path: String) -> io::Result<Self> {
        let (config, rdr) = Config::sniff(path)?;
        let index = Indexer::index(&config, Filter::empty())?;
        Ok(Self {
            rdr,
            config,
            indexer: index,
            grid: Grid::new(),
            nav: Nav::new(),
            spinner: Spinner::new(),
            fmt: Fmt::new(),
            dirty: false,
            err: String::new(),
            size: false,
            // Filter prompt
            focus_filter_prompt: false,
            filter_prompt: FilterPrompt::new(),
            // Navigator
            navigator: None,
            // Cols
            cols: ColSize::new(),
        })
    }

    pub fn is_loading(&self) -> bool {
        self.indexer.is_loading()
    }

    pub fn refresh(&mut self) {
        let (config, rdr) = Config::sniff(self.config.path.clone()).unwrap();
        let index = Indexer::index(&config, Filter::empty()).unwrap();
        self.config = config;
        self.rdr = rdr;
        self.indexer = index;
        self.grid = Grid::new();
        self.dirty = false;
    }

    pub fn on_file_change(&mut self) {
        self.dirty = true;
    }

    pub fn on_event(&mut self, event: Event) -> bool {
        if let Event::Key(event) = event {
            self.err.clear();
            if self.size {
                let col_idx = self.nav.c_col;
                self.size = false;
                match event.code {
                    KeyCode::Esc => {}
                    KeyCode::Char('r') => self.cols.reset(),
                    KeyCode::Char('f') => self.cols.fit(),
                    KeyCode::Left | KeyCode::Char('h') => self.cols.cmd(col_idx, SizeCmd::Less),
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.cols.cmd(col_idx, SizeCmd::Constrain)
                    }
                    KeyCode::Up | KeyCode::Char('k') => self.cols.cmd(col_idx, SizeCmd::Full),
                    KeyCode::Right | KeyCode::Char('l') => self.cols.cmd(col_idx, SizeCmd::More),
                    _ => self.size = true,
                }
            } else if let Some(navigator) = &mut self.navigator {
                if let Some(nav) = navigator.on_key(event.code) {
                    self.nav = nav;
                    self.navigator = None;
                }
            } else {
                if self.focus_filter_prompt {
                    match event.code {
                        KeyCode::Esc => self.focus_filter_prompt = false,
                        KeyCode::Char(':') => {
                            self.navigator = Some(Navigator::new(self.nav.clone()))
                        }
                        code => {
                            if let Some(source) = self.filter_prompt.on_key(code) {
                                match Filter::new(
                                    source,
                                    self.indexer.headers(),
                                    self.indexer.nb_col(),
                                ) {
                                    Ok(filter) => {
                                        self.indexer =
                                            Indexer::index(&self.config, filter).unwrap();
                                        self.focus_filter_prompt = false;
                                        self.filter_prompt.on_compile();
                                    }
                                    Err(err) => self.filter_prompt.on_error(err),
                                }
                            }
                        }
                    }
                } else {
                    match event.code {
                        KeyCode::Char('q') => return true,
                        KeyCode::Char('r') => self.refresh(),
                        KeyCode::Left | KeyCode::Char('h') => self.nav.left(),
                        KeyCode::Down | KeyCode::Char('j') => self.nav.down(),
                        KeyCode::Up | KeyCode::Char('k') => self.nav.up(),
                        KeyCode::Right | KeyCode::Char('l') => self.nav.right(),
                        KeyCode::Char('/') => self.focus_filter_prompt = true,
                        KeyCode::Char('s') => self.size = true,
                        KeyCode::Char('g') => {
                            self.navigator = Some(Navigator::new(self.nav.clone()))
                        }
                        _ => {}
                    }
                }
            }
        }
        false
    }

    pub fn draw(&mut self, c: &mut Canvas) {
        let w = c.width();
        // Draw error bar
        if self.dirty {
            let msg = "File content have changed, press 'r' to refresh";
            c.top().draw(format_args!("{msg:^0$}", w), style::error());
        } else if !self.err.is_empty() {
            c.top()
                .draw(format_args!("{:^1$}", self.err, w), style::error());
        }

        // Draw prompt
        if let Some(navigator) = &self.navigator {
            navigator.draw_prompt(c);
        } else if self.focus_filter_prompt {
            self.filter_prompt.draw_prompt(c);
        }

        let nav = if let Some(navigator) = &mut self.navigator {
            navigator.nav()
        } else {
            &mut self.nav
        };

        // Sync state with indexer
        let nb_col = self.indexer.nb_col();
        let nb_row = self.indexer.nb_row();
        let is_loading = self.indexer.is_loading();
        // Get rows content
        let nb_draw_row = c.height().saturating_sub(2);
        let row_off = nav.row_offset(nb_row, nb_draw_row);
        let offsets = self.indexer.get_offsets(row_off..row_off + nb_draw_row);
        self.grid.read_rows(&offsets, &mut self.rdr).unwrap();
        let rows = self.grid.rows();
        let id_len = rows
            .last()
            .map(|(i, _)| (*i as f32 + 1.).log10() as usize + 1)
            .unwrap_or(1);
        let mut col_offset_iter = nav.col_iter(nb_col);
        let mut remaining_width = c.width() - id_len as usize - 1;
        self.cols.set_nb_cols(nb_col);
        let mut cols = Vec::new();
        while remaining_width > cols.len() * 2 {
            if let Some(offset) = col_offset_iter.next() {
                let (fields, mut stat) = rows
                    .iter()
                    .map(|(_, n)| n.get(offset).unwrap_or_default())
                    .fold(
                        (Vec::new(), ColStat::new()),
                        |(mut vec, mut stat), content| {
                            let ty = Ty::guess(content);
                            stat.add(&ty, content);
                            vec.push((ty, content));
                            (vec, stat)
                        },
                    );
                if let Some(name) = self.indexer.headers().get(offset) {
                    stat.header_name(name);
                } else {
                    stat.header_idx(offset + 1);
                }
                self.cols.register_size(offset, stat.budget());
                let allowed = self
                    .cols
                    .get_size(offset)
                    .min(remaining_width - cols.len() * 2);
                remaining_width = remaining_width.saturating_sub(allowed);
                cols.push((offset, fields, stat, allowed));
            } else {
                break;
            }
        }
        cols.sort_unstable_by_key(|(i, _, _, _)| *i); // Find a way to store col in order
        drop(col_offset_iter);

        // Draw status bar
        let mut l = c.btm();
        if self.size {
            l.draw("  SIZE  ", style::state_action());
        } else if self.navigator.is_some() {
            l.draw("  GOTO  ", style::state_action());
        } else if self.focus_filter_prompt || self.indexer.filter().is_some() {
            l.draw(" FILTER ", style::state_filter());
        } else {
            l.draw(" NORMAL ", style::state_default());
        }
        if let Some(char) = self.spinner.state(is_loading) {
            l.rdraw(
                format_args!("{:>3}%{char}", self.indexer.progress()),
                style::progress(),
            );
        }
        l.rdraw(self.fmt.amount(self.nav.cursor_col() + 1), style::primary());
        l.rdraw(':', style::secondary());
        l.rdraw(self.fmt.amount(self.nav.c_row + 1), style::primary());
        l.rdraw(" ", style::primary());
        l.rdraw(
            format_args!(" {:>3}%", ((self.nav.c_row + 1) * 100) / nb_row.max(1)),
            style::primary(),
        );
        l.draw(" ", style::primary());
        if let Some(navigator) = &self.navigator {
            navigator.draw_status(&mut l, &mut self.fmt)
        } else if let Some(filter) = self.indexer.filter() {
            FilterPrompt::draw_status(&mut l, filter);
        } else {
            l.draw(&self.config.path, style::progress());
        }

        let nav = if let Some(navigator) = &mut self.navigator {
            navigator.nav()
        } else {
            &mut self.nav
        };

        // Draw headers
        {
            let line = &mut c.top();
            line.draw(
                format_args!("{:>1$} ", '#', id_len),
                style::secondary().bold(),
            );

            for (i, _, _, budget) in &cols {
                let header = if let Some(name) = self.indexer.headers().get(*i) {
                    self.fmt.rtrim(name, *budget)
                } else {
                    self.fmt.rtrim(*i + 1, *budget)
                };
                let style = if *i == nav.cursor_col() {
                    style::selected().bold()
                } else {
                    style::primary().bold()
                };
                line.draw(format_args!("{header:<0$}", budget), style);
                line.draw("  ", style::primary());
            }
        }

        // Draw rows
        for (i, (e, _)) in rows.iter().enumerate() {
            let style = if i == nav.cursor_row() {
                style::selected()
            } else {
                style::primary()
            };
            let line = &mut c.top();
            line.draw(format_args!("{:>1$} ", *e + 1, id_len), style::secondary());
            for (_, fields, stat, budget) in &cols {
                let (ty, str) = fields[i];
                line.draw(
                    format_args!("{}  ", self.fmt.field(&ty, str, stat, *budget)),
                    style,
                );
            }
        }
    }
}

struct Grid {
    /// Rows metadata
    rows: Vec<(u32, NestedString)>,
    /// Number of fresh rows
    len: usize,
}

impl Grid {
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            len: 0,
        }
    }

    pub fn read_rows(&mut self, rows: &[(u32, u64)], rdr: &mut CsvReader) -> io::Result<()> {
        self.len = 0;
        for (row, offset) in rows {
            self.read_row(*row, *offset, rdr)?;
        }
        Ok(())
    }

    fn read_row(&mut self, line: u32, offset: u64, rdr: &mut CsvReader) -> io::Result<()> {
        if self.len == self.rows.len() {
            let mut nested = NestedString::new();
            rdr.record_at(&mut nested, offset)?;
            self.rows.push((line, nested));
        } else if let Some(pos) = self.rows.iter().position(|(l, _)| *l == line) {
            self.rows.swap(self.len, pos)
        } else {
            let (l, nested) = &mut self.rows[self.len];
            rdr.record_at(nested, offset)?;
            *l = line;
        }
        self.len += 1;
        Ok(())
    }

    pub fn rows(&self) -> &[(u32, NestedString)] {
        &self.rows[..self.len]
    }
}

trait BStrWidth {
    fn width(&self) -> usize;
}

impl BStrWidth for BStr {
    fn width(&self) -> usize {
        self.chars()
            .map(|c| c.width().unwrap_or(0))
            .fold(0, Add::add)
    }
}
