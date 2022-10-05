use std::{
    io::{self, BufRead, Seek},
    ops::Add,
    sync::Arc,
    thread,
    time::{Duration, Instant, SystemTime},
};

use bstr::{BStr, ByteSlice};
use csv_core::ReadRecordResult;
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
    let path = std::env::args().nth(1).unwrap();
    let mut app = App::open(path.clone()).unwrap();
    let mut watcher = FileWatcher::new(path.clone()).unwrap();
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

struct App {
    terminal: Terminal,
    config: Config,
    rdr: CsvReader,
    grid: Grid,
    row_off: usize,
    col_off: usize,
    index: Indexer,
    spinner: Spinner,
    dirty: bool,
}

impl App {
    pub fn open(path: String) -> io::Result<Self> {
        let (config, rdr) = Config::sniff(path)?;
        let index = Indexer::index(&config)?;
        Ok(Self {
            terminal: Terminal::new(io::stdout())?,
            rdr,
            config,
            index,
            grid: Grid::new(),
            row_off: 0,
            col_off: 0,
            spinner: Spinner::new(),
            dirty: false,
        })
    }

    pub fn is_loading(&self) -> bool {
        self.index.is_loading()
    }

    pub fn refresh(&mut self) {
        let (config, rdr) = Config::sniff(self.config.path.clone()).unwrap();
        let index = Indexer::index(&config).unwrap();
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
            match event.code {
                KeyCode::Char('q') => return true,
                KeyCode::Char('r') => self.refresh(),
                _ => {}
            }
            match event.code {
                KeyCode::Down | KeyCode::Char('j') => self.row_off += 1,
                KeyCode::Up | KeyCode::Char('k') => self.row_off = self.row_off.saturating_sub(1),
                KeyCode::Right | KeyCode::Char('l') => self.col_off += 1,
                KeyCode::Left | KeyCode::Char('h') => self.col_off = self.col_off.saturating_sub(1),
                _ => {}
            }
        }
        false
    }

    pub fn draw(&mut self) {
        let Self {
            terminal,
            rdr,
            grid,
            row_off,
            index,
            spinner,
            config,
            col_off,
            ..
        } = self;

        terminal
            .draw(|c| {
                // Sync state with indexer
                let nb_row = index.nb_row();
                *row_off = nb_row.min(*row_off);
                let is_loading = index.is_loading();
                // Get rows content
                let offsets = index.get_offsets(*row_off..*row_off + c.height().saturating_sub(1));
                grid.read_rows(&offsets, rdr).unwrap();
                let rows = grid.rows();
                let nb_col = rows
                    .iter()
                    .map(|(_, n)| n)
                    .chain(index.headers.iter())
                    .map(|n| n.len())
                    .max()
                    .unwrap_or(0);
                *col_off = nb_col.min(*col_off);
                let rows: Vec<(usize, Vec<_>)> = index
                    .headers
                    .iter()
                    .map(|n| (0, n))
                    .chain(rows.iter().map(|(i, n)| (*i + 1, n)))
                    .map(|(i, n)| (i, n.iter().skip(*col_off).collect()))
                    .collect();

                // Compute padding
                let idx_pad = rows
                    .last()
                    .map(|(i, _)| (*i as f64).log10() as usize + 1)
                    .unwrap_or(1);
                let max = rows.iter().map(|(_, n)| n.len()).max().unwrap_or(0);
                let col_pad: Vec<_> = (0..max)
                    .map(|i| {
                        rows.iter()
                            .map(|(_, n)| n.get(i).map(|it| it.width()).unwrap_or(0))
                            .max()
                            .unwrap_or(0)
                    })
                    .collect();

                // Draw error bar
                if self.dirty {
                    let w = c.width();
                    let msg = "File content have changed, press 'r' to refresh";
                    c.top()
                        .draw(format_args!("{msg:^0$}", w), none().bg(Color::Red));
                }

                // Draw status bar
                let mut l = c.btm();
                if let Some(char) = spinner.state(is_loading) {
                    l.rdraw(char, none().fg(Color::Green));
                }
                l.rdraw(fmt::quantity(nb_row), none());
                l.rdraw(':', grey());
                l.rdraw(fmt::quantity(*row_off + 1), none());
                l.rdraw(' ', none());
                l.draw(&config.path, none().fg(Color::Green));

                // Draw rows bar
                for (l, n) in rows.into_iter() {
                    let mut line = c.top();
                    let style = if l == 0 {
                        line.draw(
                            format_args!("{:<1$} ", '#', idx_pad),
                            none().fg(Color::DarkGrey).bold(),
                        );
                        none().fg(Color::Blue).bold()
                    } else {
                        line.draw(
                            format_args!("{:<1$} ", l, idx_pad),
                            none().fg(Color::DarkGrey),
                        );
                        none()
                    };

                    let mut cols = col_pad.iter().enumerate();
                    while line.width() > 0 {
                        if let Some((i, pad)) = cols.next() {
                            let content = n
                                .get(i)
                                .map(|it| *it)
                                .unwrap_or_else(|| BStr::new(if l == 0 { "?" } else { "" }));
                            line.draw(format_args!("{content:<0$} ", pad), style);
                        } else {
                            break;
                        }
                    }
                }
            })
            .unwrap();
    }
}

trait BStrWidth {
    fn width(&self) -> usize;
}

impl BStrWidth for BStr {
    fn width(&self) -> usize {
        self.chars()
            .map(|c| c.width_cjk().unwrap_or(0))
            .fold(0, Add::add)
    }
}

struct Grid {
    /// Rows metadata
    rows: Vec<(usize, NestedString)>,
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

    pub fn read_rows(&mut self, rows: &[(usize, u64)], rdr: &mut CsvReader) -> io::Result<()> {
        self.len = 0;
        for (row, offset) in rows {
            self.read_row(*row, *offset, rdr)?;
        }
        Ok(())
    }

    fn read_row(&mut self, line: usize, offset: u64, rdr: &mut CsvReader) -> io::Result<()> {
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

    pub fn rows(&self) -> &[(usize, NestedString)] {
        &self.rows[..self.len]
    }
}

struct IndexerState {
    index: Vec<u64>,
}

pub struct Indexer {
    pub headers: Option<NestedString>,
    task: Arc<Mutex<IndexerState>>,
    // TODO show error
}

impl Indexer {
    pub fn index(config: &Config) -> io::Result<Self> {
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
            thread::spawn(|| Self::bg_index(rdr, task));
        }

        Ok(Self { headers, task })
    }

    fn bg_index(mut rdr: CsvReader, task: Arc<Mutex<IndexerState>>) -> io::Result<()> {
        let (file, rdr) = rdr.inner_mut();

        // Dummy ignored buffer
        let mut out = [0; BUF_LEN];
        let mut bounds = [0; 100];

        let mut pos = file.stream_position()?;
        let mut buff_pos = vec![pos];

        loop {
            let buff = file.fill_buf()?;
            let (result, amount, _, _) = rdr.read_record(buff, &mut out, &mut bounds);
            pos += amount as u64;
            file.consume(amount);
            match result {
                ReadRecordResult::InputEmpty
                | ReadRecordResult::OutputFull
                | ReadRecordResult::OutputEndsFull => continue, // We ignore outputs
                ReadRecordResult::Record => {
                    // Throttle locking
                    if buff_pos.len() == 100 {
                        // If arc is unique this task is canceled
                        if Arc::strong_count(&task) == 1 {
                            return Ok(());
                        }
                        task.lock().index.append(&mut buff_pos)
                    }
                    buff_pos.push(pos);
                }
                ReadRecordResult::End => break,
            }
        }
        buff_pos.pop();
        {
            // Finalize state
            task.lock().index.append(&mut buff_pos);
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
    pub fn get_offsets(&self, rows: impl Iterator<Item = usize>) -> Vec<(usize, u64)> {
        let lock = self.task.lock();
        rows.map_while(|i| lock.index.get(i).map(|p| (i, *p)))
            .collect()
    }
}
