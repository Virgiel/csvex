use std::{
    io::{self},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed},
        Arc,
    },
    thread,
};

use parking_lot::Mutex;

use crate::{
    filter::{Engine, Filter},
    read::{Config, CsvReader, NestedString},
};

struct State {
    index: Mutex<Vec<(u32, u64)>>,
    headers: NestedString,
    filter: Filter,
    file_len: u64,
    nb_col: AtomicUsize,
    nb_read: AtomicU64,
    // TODO store indexer error
}

pub struct Indexer {
    state: Arc<State>,
}

impl Indexer {
    pub fn index(config: &Config, filter: Filter) -> io::Result<Self> {
        let mut rdr = config.reader()?;
        let mut headers = NestedString::new();
        if config.has_header {
            rdr.record(&mut headers)?;
        }
        let state = Arc::new(State {
            index: Mutex::new(Vec::with_capacity(1000)),
            filter,
            file_len: rdr.len()?,
            nb_col: AtomicUsize::new(headers.len()),
            nb_read: AtomicU64::new(rdr.pos()?),
            headers,
        });

        {
            let state = state.clone();
            thread::spawn(|| Self::bg_index(rdr, state));
        }

        Ok(Self { state })
    }

    fn bg_index(mut rdr: CsvReader, state: Arc<State>) -> io::Result<()> {
        let engine = Engine::new(&state.filter);
        let mut record = NestedString::new();
        let mut buff_pos = Vec::with_capacity(100);
        let mut pos = state.nb_read.load(Relaxed);
        let mut max_col = state.nb_col.load(Relaxed);

        let mut count = 0;
        loop {
            let amount = rdr.record(&mut record)?;
            if amount == 0 {
                break;
            } else if engine.check(&record) {
                state.index.lock().push((count, pos));
            }

            pos += amount as u64;
            count += 1;
            max_col = max_col.max(record.len());

            // Throttle locking
            if count % 1000 == 0 {
                // If arc is unique this task is canceled
                if Arc::strong_count(&state) == 1 {
                    return Ok(());
                }
                if !buff_pos.is_empty() {
                    state.index.lock().append(&mut buff_pos);
                }
                state.nb_col.store(max_col, Relaxed);
                state.nb_read.store(pos, Relaxed);
            }
        }

        Ok(())
    }

    // Check if the indexer is working in the background
    pub fn is_loading(&self) -> bool {
        Arc::strong_count(&self.state) > 1
    }

    /// Get number of indexed rows
    pub fn nb_row(&self) -> usize {
        self.state.index.lock().len()
    }

    /// Get offsets of given rows
    pub fn get_offsets(&self, rows: impl Iterator<Item = usize>) -> Vec<(u32, u64)> {
        let locked = self.state.index.lock();
        rows.map_while(|i| locked.get(i).copied()).collect()
    }

    pub fn filter(&self) -> Option<&str> {
        (!self.state.filter.nodes.is_empty()).then_some(self.state.filter.source.as_str())
    }

    pub fn headers(&self) -> &NestedString {
        &self.state.headers
    }

    pub fn nb_col(&self) -> usize {
        self.state.nb_col.load(Relaxed)
    }

    pub fn progress(&self) -> u8 {
        (self.state.nb_read.load(Relaxed) * 100 / self.state.file_len.max(1)) as u8
    }
}
