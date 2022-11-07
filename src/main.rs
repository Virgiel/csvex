use std::{
    io::{self},
    ops::Add,
    path::PathBuf,
    time::Duration,
};

use bstr::{BStr, ByteSlice};
use clap::Parser;
use cols::{Cols, ColsCmd, SizeCmd};
use filter::Filter;
use fmt::{ColStat, Fmt, Ty};
use histogram::Histographer;
use index::Indexer;
use nav::Nav;
use reader::{CsvReader, NestedString};
use source::Source;
use spinner::Spinner;
use tui::{
    crossterm::event::{self, Event, KeyCode, KeyModifiers},
    unicode_width::UnicodeWidthChar,
    Canvas, Terminal,
};
use ui::{FilterPrompt, Navigator};

mod cols;
mod filter;
mod fmt;
mod histogram;
mod index;
mod nav;
mod prompt;
mod reader;
mod source;
mod spinner;
mod style;
mod ui;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub const BUF_LEN: usize = 8 * 1024;

#[derive(clap::Parser, Debug)]
pub struct Args {
    pub filename: Option<PathBuf>,
}

pub fn nb_print_len(nb: usize) -> usize {
    (nb as f64).log10() as usize + 1
}

fn main() {
    let args = Args::parse();
    let mut app = App::open(args.filename).unwrap();
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
        if is_loading {
            redraw = true;
        }
    }
}

enum AppState {
    Normal,
    Filter { show_off: bool },
    Size,
    Nav(Navigator),
    Histogram(Histographer),
}

enum GridType<'a> {
    Normal {
        id_len: usize,
        cols: Vec<(usize, Vec<(Ty, &'a BStr)>, ColStat, usize)>,
        rows: &'a [(u32, NestedString)],
    },
    Histogram,
}

struct App {
    source: Source,
    rdr: CsvReader,
    grid: Grid,
    nav: Nav,
    indexer: Indexer,
    spinner: Spinner,
    fmt: Fmt,
    dirty: bool,
    err: String,
    cols: Cols,
    state: AppState,
    filter_prompt: FilterPrompt,
}

impl App {
    pub fn open(filename: Option<PathBuf>) -> io::Result<Self> {
        let (source, rdr) = Source::new(filename)?;
        let (headers, index) = Indexer::index(&source, Filter::empty())?;
        Ok(Self {
            source,
            rdr,
            indexer: index,
            grid: Grid::new(),
            nav: Nav::new(),
            spinner: Spinner::new(),
            fmt: Fmt::new(),
            dirty: false,
            err: String::new(),
            cols: Cols::new(headers),
            filter_prompt: FilterPrompt::new(),
            state: AppState::Normal,
        })
    }

    pub fn is_loading(&self) -> bool {
        match &self.state {
            AppState::Histogram(h) => h.is_loading(),
            _ => self.indexer.is_loading(),
        }
    }

    pub fn refresh(&mut self) {
        let rdr = self.source.refresh().unwrap();
        let (headers, index) = Indexer::index(&self.source, Filter::empty()).unwrap();
        self.rdr = rdr;
        self.indexer = index;
        self.cols.set_headers(headers);
        self.grid = Grid::new();
        self.dirty = false;
        if let AppState::Histogram(h) = &mut self.state {
            let (off, _) = self.cols.get_col(self.nav.c_col);
            *h = Histographer::analyze(&self.source, off, self.indexer.filter().clone()).unwrap();
        }
    }

    pub fn on_event(&mut self, event: Event) -> bool {
        if let Event::Key(event) = event {
            self.err.clear();

            match &mut self.state {
                AppState::Normal => match event.code {
                    KeyCode::Char('q') => return true,
                    KeyCode::Char('r') => self.refresh(),
                    KeyCode::Char('-') => {
                        self.cols.cmd(self.nav.c_col, ColsCmd::Hide);
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        if event.modifiers.contains(KeyModifiers::SHIFT) {
                            self.cols.cmd(self.nav.c_col, ColsCmd::Left);
                        }
                        self.nav.left()
                    }
                    KeyCode::Down | KeyCode::Char('j') => self.nav.down(),
                    KeyCode::Up | KeyCode::Char('k') => self.nav.up(),
                    KeyCode::Right | KeyCode::Char('l') => {
                        if event.modifiers.contains(KeyModifiers::SHIFT) {
                            self.cols.cmd(self.nav.c_col, ColsCmd::Right);
                        }
                        self.nav.right()
                    }
                    KeyCode::Char('/') => self.state = AppState::Filter { show_off: true },
                    KeyCode::Char('s') => self.state = AppState::Size,
                    KeyCode::Char('g') => {
                        self.state = AppState::Nav(Navigator::new(self.nav.clone()))
                    }
                    KeyCode::Char('f') => {
                        let (off, _) = self.cols.get_col(self.nav.c_col);
                        self.state = AppState::Histogram(
                            Histographer::analyze(&self.source, off, self.indexer.filter().clone())
                                .unwrap(),
                        )
                    }
                    _ => {}
                },
                AppState::Filter { show_off } => match event.code {
                    KeyCode::Esc => self.state = AppState::Normal,
                    KeyCode::Tab => *show_off = !*show_off,
                    code => {
                        let (source, apply) = self.filter_prompt.on_key(code);
                        match Filter::new(source, self.cols.nb_col()) {
                            Ok(filter) => {
                                if apply {
                                    let (headers, index) =
                                        Indexer::index(&self.source, filter).unwrap();
                                    self.indexer = index;
                                    self.cols.set_headers(headers);
                                    self.state = AppState::Normal;
                                    self.filter_prompt.on_compile();
                                }
                            }
                            Err(err) => self.filter_prompt.on_error(err, apply),
                        }
                    }
                },
                AppState::Size => {
                    let col_idx = self.nav.c_col;
                    let mut exit_size = true;
                    match event.code {
                        KeyCode::Esc => {}
                        KeyCode::Char('r') => self.cols.reset_size(),
                        KeyCode::Char('f') => self.cols.fit(),
                        KeyCode::Left | KeyCode::Char('h') => {
                            self.cols.size_cmd(col_idx, SizeCmd::Less)
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            self.cols.size_cmd(col_idx, SizeCmd::Constrain)
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            self.cols.size_cmd(col_idx, SizeCmd::Full)
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            self.cols.size_cmd(col_idx, SizeCmd::More)
                        }
                        _ => exit_size = false,
                    };
                    if exit_size {
                        self.state = AppState::Normal
                    }
                }
                AppState::Nav(navigator) => {
                    if let Some(nav) = navigator.on_key(event.code) {
                        self.nav = nav;
                        self.state = AppState::Normal;
                    }
                }
                AppState::Histogram(h) => match event.code {
                    KeyCode::Esc => self.state = AppState::Normal,
                    KeyCode::Down | KeyCode::Char('j') => h.down(),
                    KeyCode::Up | KeyCode::Char('k') => h.up(),
                    _ => {}
                },
            }
        }
        false
    }

    pub fn draw(&mut self, c: &mut Canvas) {
        if !self.dirty {
            self.dirty = self.source.check_dirty().unwrap();
        }

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
        match &self.state {
            AppState::Filter { .. } => self.filter_prompt.draw_prompt(c),
            AppState::Nav(navigator) => {
                navigator.draw_prompt(c);
            }
            AppState::Normal | AppState::Size | AppState::Histogram(_) => {}
        }

        let nb_draw_row = c.height().saturating_sub(2);
        let (progress, ty) = match &mut self.state {
            AppState::Histogram(h) => (h.ui_progress(nb_draw_row), GridType::Histogram),
            _ => {
                let nav = match &mut self.state {
                    AppState::Nav(navigator) => navigator.nav(),
                    _ => &mut self.nav,
                };
                // Sync state with indexer
                let nb_col = self.indexer.nb_col();
                let nb_row = self.indexer.nb_row();
                self.cols.set_nb_cols(nb_col);
                let visible_cols = self.cols.visible_col();
                // Get rows content
                let row_off = nav.row_offset(nb_row, nb_draw_row);
                let offsets = self.indexer.get_offsets(row_off..row_off + nb_draw_row);
                self.grid.read_rows(&offsets, &mut self.rdr).unwrap();
                let rows = self.grid.rows();
                let id_len = rows
                    .last()
                    .map(|(i, _)| (*i as f32 + 1.).log10() as usize + 1)
                    .unwrap_or(1);
                let mut remain_table_w = c.width() - id_len as usize - 1;
                let mut cols = Vec::new();
                nav.col_iter(visible_cols, |idx| {
                    let remain_col_w = remain_table_w.saturating_sub(cols.len());
                    if remain_col_w > 0 {
                        let (fields, mut stat) = rows
                            .iter()
                            .map(|(_, n)| n.get(self.cols.get_col(idx).0).unwrap_or_default())
                            .fold(
                                (Vec::new(), ColStat::new()),
                                |(mut vec, mut stat), content| {
                                    let ty = Ty::guess(content);
                                    stat.add(&ty, content);
                                    vec.push((ty, content));
                                    (vec, stat)
                                },
                            );
                        let name = self.cols.get_col(idx).1;
                        stat.header_name(name);
                        let col_size = self.cols.size(idx, stat.budget());
                        let allowed = col_size.min(remain_col_w);
                        remain_table_w = remain_table_w.saturating_sub(allowed);
                        cols.push((idx, fields, stat, allowed));
                        remain_col_w >= col_size
                    } else {
                        false
                    }
                });
                cols.sort_unstable_by_key(|(i, _, _, _)| *i); // Find a way to store col in order
                (
                    ((self.nav.c_row + 1) * 100) / nb_row.max(1),
                    GridType::Normal { id_len, cols, rows },
                )
            }
        };

        // Draw status bar
        let mut l = c.btm();
        match &self.state {
            AppState::Filter { .. } => l.draw(" FILTER ", style::state_alternate()),
            AppState::Normal if self.indexer.filter_string().is_some() => {
                l.draw(" FILTER ", style::state_alternate())
            }
            AppState::Normal => l.draw(" NORMAL ", style::state_default()),
            AppState::Size => l.draw("  SIZE  ", style::state_action()),
            AppState::Nav(_) => l.draw("  GOTO  ", style::state_action()),
            AppState::Histogram(_) => l.draw("  FREQ  ", style::state_alternate()),
        };
        l.draw(" ", style::primary());

        if let Some(char) = self.spinner.state(self.is_loading()) {
            let progress = match &self.state {
                AppState::Histogram(h) => h.progress(),
                _ => self.indexer.progress(),
            };
            l.rdraw(format_args!(" {:>2}%{char}", progress), style::progress());
        } else {
            l.rdraw(format_args!(" {progress:>3}%"), style::primary());
        }

        if self.cols.nb_col() > 0 {
            let (_, name) = self.cols.get_col(self.nav.c_col);
            l.rdraw(name, style::primary());
            l.rdraw(" ", style::primary());
        }

        match &self.state {
            AppState::Nav(navigator) => navigator.draw_status(&mut l, &mut self.fmt),
            _ => {
                if let Some(filter) = self.indexer.filter_string() {
                    FilterPrompt::draw_status(&mut l, filter)
                } else {
                    l.draw(&self.source.display_path, style::progress());
                }
            }
        }

        match ty {
            GridType::Normal { id_len, cols, rows } => {
                // Draw headers
                let show_off = matches!(self.state, AppState::Filter { show_off: true });
                let nav = match &mut self.state {
                    AppState::Nav(navigator) => navigator.nav(),
                    _ => &mut self.nav,
                };
                if self.source.has_header || show_off {
                    let line = &mut c.top();
                    line.draw(
                        format_args!("{:>1$} ", '#', id_len),
                        style::secondary().bold(),
                    );

                    for (i, _, _, budget) in &cols {
                        let (off, name) = self.cols.get_col(*i);

                        if show_off {
                            let style = if *i == nav.c_col {
                                style::selected().bold()
                            } else {
                                style::secondary().bold()
                            };
                            line.draw(format_args!("{off:<0$}", budget), style);
                        } else {
                            let style = if *i == nav.c_col {
                                style::selected().bold()
                            } else {
                                style::primary().bold()
                            };
                            line.draw(
                                format_args!("{:<1$}", self.fmt.rtrim(name, *budget), budget),
                                style,
                            );
                        }
                        line.draw("│", style::separator());
                    }
                }

                // Draw rows
                for (i, (e, _)) in rows.iter().enumerate() {
                    let style = if i == nav.c_row - nav.o_row {
                        style::selected()
                    } else {
                        style::primary()
                    };
                    let line = &mut c.top();
                    line.draw(format_args!("{:>1$} ", *e + 1, id_len), style::secondary());
                    for (_, fields, stat, budget) in &cols {
                        let (ty, str) = fields[i];
                        line.draw(
                            format_args!("{}", self.fmt.field(&ty, str, stat, *budget)),
                            style,
                        );
                        line.draw("│", style::separator());
                    }
                }
            }
            GridType::Histogram => {
                if let AppState::Histogram(h) = &mut self.state {
                    h.draw_grid(c, &mut self.fmt)
                } else {
                    unreachable!()
                }
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
