use std::{
    io::{self, Seek},
    sync::Arc,
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
            headers,
            filter,
        });

        {
            let state = state.clone();
            thread::spawn(|| Self::bg_index(rdr, state));
        }

        Ok(Self { state })
    }

    fn bg_index(mut rdr: CsvReader, state: Arc<State>) -> io::Result<()> {
        // Slower filtering indexer
        let engine = Engine::new(&state.filter, &state.headers);
        let mut record = NestedString::new();
        let mut pos = rdr.file.stream_position()?;
        let mut buff_pos = Vec::with_capacity(100);

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

            // Throttle locking
            if count % 100 == 0 {
                // If arc is unique this task is canceled
                if Arc::strong_count(&state) == 1 {
                    return Ok(());
                }
                if !buff_pos.is_empty() {
                    state.index.lock().append(&mut buff_pos);
                }
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
}
