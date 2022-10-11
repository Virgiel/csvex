use std::{
    io::{self, Seek},
    ops::Add,
    sync::Arc,
    thread,
    time::{Duration, Instant, SystemTime},
};

use bstr::{BStr, ByteSlice};
use filter::{Engine, Filter, Highlighter, Style};
use fmt::{fmt_field, rtrim, ColStat, FmtBuffer, Ty};
use parking_lot::Mutex;
use read::{Config, CsvReader, NestedString};
use spinner::Spinner;
use style::grey;
use tui::{
    crossterm::event::{self, Event, KeyCode},
    none,
    unicode_width::UnicodeWidthChar,
    Color, Terminal,
};

mod filter;
mod fmt;
mod read;
mod spinner;
mod style;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub const BUF_LEN: usize = 8 * 1024;

pub const WATCHER_POOL: Duration = Duration::from_secs(1);
struct FileWatcher {
    path: String,
    m_time: SystemTime,
    last: Instant,
}

impl FileWatcher {
    pub fn new(path: String) -> io::Result<Self> {
        Ok(Self {
            last: Instant::now(),
            m_time: std::fs::metadata(&path)?.modified()?,
            path,
        })
    }

    pub fn has_change(&mut self) -> io::Result<bool> {
        Ok(if self.last.elapsed() < WATCHER_POOL {
            false
        } else {
            self.last = Instant::now();
            let m_time = std::fs::metadata(&self.path)?.modified()?;
            if m_time != self.m_time {
                self.m_time = m_time;
                true
            } else {
                false
            }
        })
    }
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or("../../Downloads/adresses-france.csv".to_string());
    let mut app = App::open(path.clone()).unwrap();
    let mut watcher = FileWatcher::new(path).unwrap();
    let mut redraw = true;
    loop {
        // Check loading state before drawing to no skip completed task during drawing
        let is_loading = app.is_loading();
        if redraw {
            app.draw();
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

struct Nav {
    // Offset position
    o_row: usize,
    o_col: usize,
    // Cursor positions
    c_row: usize,
    c_col: usize,
    // View dimension
    v_row: usize,
    v_col: usize,
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

    pub fn row_offset(&mut self, total: usize, nb: usize) -> usize {
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
}

struct App {
    terminal: Terminal,
    config: Config,
    rdr: CsvReader,
    grid: Grid,
    nav: Nav,
    index: Indexer,
    spinner: Spinner,
    fmt_buff: FmtBuffer,
    dirty: bool,
    filter: Option<String>,
    err: String,
}

impl App {
    pub fn open(path: String) -> io::Result<Self> {
        let (config, rdr) = Config::sniff(path)?;
        let index = Indexer::index(&config, Filter::empty())?;
        Ok(Self {
            terminal: Terminal::new(io::stdout())?,
            rdr,
            config,
            index,
            grid: Grid::new(),
            nav: Nav::new(),
            spinner: Spinner::new(),
            fmt_buff: FmtBuffer::new(),
            dirty: false,
            filter: None,
            err: String::new(),
        })
    }

    pub fn is_loading(&self) -> bool {
        self.index.is_loading()
    }

    pub fn refresh(&mut self) {
        let (config, rdr) = Config::sniff(self.config.path.clone()).unwrap();
        let index = Indexer::index(&config, self.index.filter.clone()).unwrap();
        self.config = config;
        self.rdr = rdr;
        self.index = index;
        self.grid = Grid::new();
        self.dirty = false;
    }

    pub fn on_file_change(&mut self) {
        self.dirty = true;
    }

    pub fn on_event(&mut self, event: Event) -> bool {
        if let Event::Key(event) = event {
            self.err.clear();
            match &mut self.filter {
                Some(filter) => match event.code {
                    KeyCode::Esc => {
                        self.filter = None;
                        if !self.index.filter.source.is_empty() {
                            self.index = Indexer::index(&self.config, Filter::empty()).unwrap()
                        }
                    }
                    KeyCode::Char(c) => filter.push(c),
                    KeyCode::Backspace => {
                        filter.pop();
                    }
                    KeyCode::Enter => match Filter::new(filter.clone()) {
                        Ok(it) => self.index = Indexer::index(&self.config, it).unwrap(),
                        Err((pos, msg)) => self.err = format!("at {pos:?}: {msg}"),
                    },
                    _ => {}
                },
                None => {
                    match event.code {
                        KeyCode::Char('q') => return true,
                        KeyCode::Char('r') => self.refresh(),
                        _ => {}
                    }
                    match event.code {
                        KeyCode::Left | KeyCode::Char('h') => self.nav.left(),
                        KeyCode::Down | KeyCode::Char('j') => self.nav.down(),
                        KeyCode::Up | KeyCode::Char('k') => self.nav.up(),
                        KeyCode::Right | KeyCode::Char('l') => self.nav.right(),
                        KeyCode::Char('/') => self.filter = Some(String::new()),
                        _ => {}
                    }
                }
            }
        }
        false
    }

    pub fn draw(&mut self) {
        let Self {
            terminal,
            rdr,
            grid,
            nav,
            index,
            spinner,
            config,
            fmt_buff,
            ..
        } = self;

        terminal
            .draw(|c| {
                let w = c.width();
                // Draw error bar
                if self.dirty {
                    let msg = "File content have changed, press 'r' to refresh";
                    c.top()
                        .draw(format_args!("{msg:^0$}", w), none().fg(Color::Red));
                } else if !self.err.is_empty() {
                    c.top()
                        .draw(format_args!("{:^1$}", self.err, w), none().fg(Color::Red));
                }

                // Sync state with indexer
                let nb_row = index.nb_row();
                let is_loading = index.is_loading();
                // Get rows content
                let nb_draw_row = c.height().saturating_sub(2);
                let row_off = nav.row_offset(nb_row, nb_draw_row);
                let offsets = index.get_offsets(row_off..row_off + nb_draw_row);
                grid.read_rows(&offsets, rdr).unwrap();
                let rows = grid.rows();
                let nb_col = rows
                    .iter()
                    .map(|(_, n)| n)
                    .chain(index.headers.iter())
                    .map(|n| n.len())
                    .max()
                    .unwrap_or(0);
                let id_len = rows
                    .last()
                    .map(|(i, _)| (*i as f32).log10() as usize + 1)
                    .unwrap_or(1);
                let mut col_offset_iter = nav.col_iter(nb_col);
                let mut remaining_width = c.width() - id_len as usize - 1;
                let mut cols = Vec::new();
                while remaining_width > cols.len() {
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
                        if let Some(headers) = &index.headers {
                            stat.header_name(headers.get(offset).unwrap_or_else(|| BStr::new("?")));
                        } else {
                            stat.header_idx(offset + 1);
                        }
                        let allowed = stat.budget().min(remaining_width - cols.len());
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
                if let Some(filter) = &self.filter {
                    l.draw("filter: ", none());
                    for win in Highlighter::new(filter).styles.windows(2) {
                        match win {
                            [(start, _), (end, style)] => {
                                l.draw(
                                    &filter[*start..*end],
                                    match style {
                                        Style::None => none(),
                                        Style::Id => none().fg(Color::Blue),
                                        Style::Nb => none().fg(Color::Yellow),
                                        Style::Str => none().fg(Color::Green),
                                        Style::Regex => none().fg(Color::Magenta),
                                        Style::Action => none().fg(Color::Red),
                                        Style::Logi => none().fg(Color::Cyan),
                                    },
                                );
                            }
                            _ => unreachable!(),
                        }
                    }
                    l.cursor();
                } else {
                    if let Some(char) = spinner.state(is_loading) {
                        l.rdraw(char, none().fg(Color::Green));
                    }
                    l.rdraw(fmt::quantity(fmt_buff, nb_row), none());
                    l.rdraw(':', grey());
                    l.rdraw(fmt::quantity(fmt_buff, nb_col), none());
                    l.rdraw('|', grey());
                    l.rdraw(fmt::quantity(fmt_buff, nav.o_row + 1), none());
                    l.rdraw(':', grey());
                    l.rdraw(fmt::quantity(fmt_buff, nav.cursor_col() + 1), none());
                    l.rdraw(' ', none());
                    l.draw(&config.path, none().fg(Color::Green));
                }

                // Draw headers
                {
                    let line = &mut c.top();
                    line.draw(
                        format_args!("{:>1$} ", '#', id_len),
                        none().fg(Color::DarkGrey).bold(),
                    );

                    for (i, _, _, budget) in &cols {
                        let header = if let Some(header) = &index.headers {
                            let name = header.get(*i).unwrap_or_else(|| BStr::new("?"));
                            rtrim(name, fmt_buff, *budget)
                        } else {
                            rtrim(*i + 1, fmt_buff, *budget)
                        };
                        let style = if *i == nav.cursor_col() {
                            style::reverse(none().bold())
                        } else {
                            none().bold()
                        };
                        line.draw(format_args!("{header:<0$} ", budget), style);
                    }
                }

                // Draw rows
                for (i, (e, _)) in rows.iter().enumerate() {
                    let style = if i == nav.cursor_row() {
                        style::reverse(none())
                    } else {
                        none()
                    };
                    let line = &mut c.top();
                    line.draw(
                        format_args!("{:>1$} ", *e, id_len),
                        none().fg(Color::DarkGrey),
                    );
                    for (_, fields, stat, budget) in &cols {
                        let (ty, str) = fields[i];
                        line.draw(
                            format_args!("{} ", fmt_field(fmt_buff, &ty, str, stat, *budget)),
                            style,
                        );
                    }
                }
            })
            .unwrap();
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

struct IndexerState {
    index: Vec<(u32, u64)>,
}

pub struct Indexer {
    pub headers: Option<NestedString>,
    task: Arc<Mutex<IndexerState>>,
    filter: Arc<Filter>, // TODO show error
}

impl Indexer {
    pub fn index(config: &Config, filter: Arc<Filter>) -> io::Result<Self> {
        let mut rdr = config.reader()?;
        let headers = if config.has_header {
            let mut nested = NestedString::new();
            rdr.record(&mut nested)?;
            Some(nested)
        } else {
            None
        };
        let task = Arc::new(Mutex::new(IndexerState { index: vec![] }));
        {
            let task = task.clone();
            let filter = filter.clone();
            thread::spawn(|| Self::bg_index(rdr, filter, task));
        }

        Ok(Self {
            headers,
            task,
            filter,
        })
    }

    fn bg_index(
        mut rdr: CsvReader,
        filter: Arc<Filter>,
        task: Arc<Mutex<IndexerState>>,
    ) -> io::Result<()> {
        // Slower filtering indexer
        let engine = Engine::new(&filter);
        let mut record = NestedString::new();
        let mut pos = rdr.file.stream_position()?;
        let mut buff_pos = Vec::with_capacity(100);

        let mut count = 0;
        loop {
            let amount = rdr.record(&mut record)?;
            if amount == 0 {
                break;
            } else if engine.check(&record) {
                task.lock().index.push((count, pos));
            }

            pos += amount as u64;
            count += 1;

            // Throttle locking
            if count % 100 == 0 {
                // If arc is unique this task is canceled
                if Arc::strong_count(&task) == 1 {
                    return Ok(());
                }
                if !buff_pos.is_empty() {
                    task.lock().index.append(&mut buff_pos);
                }
            }
        }

        Ok(())
    }

    // Check if the indexer is working in the background
    pub fn is_loading(&self) -> bool {
        Arc::strong_count(&self.task) == 2
    }

    /// Get number of indexed rows
    pub fn nb_row(&self) -> usize {
        self.task.lock().index.len()
    }

    /// Get offsets of given rows
    pub fn get_offsets(&self, rows: impl Iterator<Item = usize>) -> Vec<(u32, u64)> {
        let locked = self.task.lock();
        rows.map_while(|i| locked.index.get(i).copied()).collect()
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
