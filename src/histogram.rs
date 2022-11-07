use std::{
    io,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed},
        Arc,
    },
    thread,
};

use bstr::{BStr, BString, ByteSlice};
use indexmap::IndexMap;
use parking_lot::Mutex;
use tui::Canvas;

use crate::{
    filter::{Engine, Filter},
    fmt::{ColStat, Fmt, Ty},
    nb_print_len,
    reader::{CsvReader, NestedString},
    source::Source,
    style, Nav,
};

struct Histogram {
    /// Map value to their count index
    values: IndexMap<BString, usize>,
    /// Occurrence count
    counts: Vec<(usize, u64)>,
}

impl Histogram {
    pub fn new() -> Self {
        Self {
            values: IndexMap::new(),
            counts: Vec::new(),
        }
    }

    pub fn register(&mut self, value: &BStr) -> usize {
        if let Some(count_idx) = self.values.get(value).map(|i| *i) {
            // Increment count
            let (value_idx, mut count) = self.counts[count_idx];
            count += 1;
            self.counts[count_idx] = (value_idx, count);
            // Check if sorted
            if count_idx != 0 && self.counts[count_idx - 1].1 < count {
                // Find place to swap
                let swap_idx = self.counts[..count_idx]
                    .iter()
                    .rposition(|(_, c)| *c >= count)
                    .map(|p| p + 1)
                    .unwrap_or(0);
                // Swap
                self.counts.swap(count_idx, swap_idx);
                *self.values.get_index_mut(value_idx).unwrap().1 = swap_idx;
                *self
                    .values
                    .get_index_mut(self.counts[count_idx].0)
                    .unwrap()
                    .1 = count_idx;
            }
        } else {
            // Add new
            let (value_idx, _) = self.values.insert_full(value.into(), self.counts.len());
            self.counts.push((value_idx, 1));
        }
        self.counts.len()
    }

    pub fn items<'a>(
        &'a self,
    ) -> impl Iterator<Item = (&'a BStr, u64)> + ExactSizeIterator + Clone + 'a {
        self.counts
            .iter()
            .map(|(idx, count)| (BStr::new(self.values.get_index(*idx).unwrap().0), *count))
    }
}

struct State {
    histogram: Mutex<Histogram>,
    file_len: u64,
    nb_read: AtomicU64,
    nb_item: AtomicU64,
    nb_row: AtomicUsize, // TODO store indexer error
}
pub struct Histographer {
    name: String,
    state: Arc<State>,
    nav: Nav,
}

impl Histographer {
    pub fn analyze(source: &Source, off: usize, filter: Filter) -> io::Result<Self> {
        let (mut rdr, headers) = source.reader()?;
        let name = headers.get(off).unwrap_or_default().to_string();
        let state = Arc::new(State {
            file_len: rdr.len()?,
            nb_read: AtomicU64::new(rdr.pos()?),
            nb_item: AtomicU64::new(0),
            nb_row: AtomicUsize::new(0),
            histogram: Mutex::new(Histogram::new()),
        });

        {
            let state = state.clone();
            thread::spawn(move || Self::bg_analyze(rdr, off, filter, state));
        }

        Ok(Self {
            name,
            state,
            nav: Nav::new(),
        })
    }

    fn bg_analyze(
        mut rdr: CsvReader,
        idx: usize,
        filter: Filter,
        state: Arc<State>,
    ) -> io::Result<()> {
        let engine = Engine::new(&filter);
        let mut record = NestedString::new();
        loop {
            let amount = rdr.record(&mut record)?;
            if amount == 0 {
                break;
            } else if Arc::strong_count(&state) == 1 {
                return Ok(());
            }
            state.nb_read.fetch_add(amount as u64, Relaxed);

            if engine.check(&record) {
                let col = BStr::new(record.get(idx).unwrap_or_default().trim());
                let nb_row = state.histogram.lock().register(col);
                state.nb_row.store(nb_row, Relaxed);
                state.nb_item.fetch_add(1, Relaxed);
            }
        }
        Ok(())
    }

    // Check if the indexer is working in the background
    pub fn is_loading(&self) -> bool {
        Arc::strong_count(&self.state) > 1
    }

    pub fn progress(&self) -> u8 {
        (self.state.nb_read.load(Relaxed) * 100 / self.state.file_len.max(1)) as u8
    }

    pub fn up(&mut self) {
        self.nav.up()
    }

    pub fn down(&mut self) {
        self.nav.down()
    }

    pub fn ui_progress(&mut self, nb_show: usize) -> usize {
        let nb_row = self.state.nb_row.load(Relaxed);
        self.nav.row_offset(nb_row, nb_show);
        ((self.nav.c_row + 1) * 100) / nb_row.max(1)
    }

    pub fn draw_grid(&mut self, c: &mut Canvas, fmt: &mut Fmt) {
        let nb_row = self.state.nb_row.load(Relaxed);
        let nb_item = self.state.nb_item.load(Relaxed);
        let offset = self.nav.row_offset(nb_row, c.height() - 1);
        let locked = self.state.histogram.lock();
        let rows = locked.items().skip(offset).take(c.height());
        let (rows, mut stat) = rows.clone().fold(
            (Vec::new(), ColStat::new()),
            |(mut vec, mut stat), (v, count)| {
                let ty = Ty::guess(v);
                stat.add(&ty, v);
                vec.push((v, count, ty));

                (vec, stat)
            },
        );
        stat.header_name(BStr::new(&self.name));
        let max = locked.items().next().map(|(_, c)| c).unwrap_or(0);
        let nb_budget = nb_print_len(max as usize).max(5);
        // Draw headers
        let mut l = c.top();
        let budget = stat.budget();
        l.draw(
            format_args!("{:<1$}", fmt.rtrim(&self.name, budget), budget),
            style::primary().bold(),
        );
        l.draw("│", style::separator());
        l.draw(
            format_args!("{:<1$}", "count", nb_budget),
            style::primary().bold(),
        );
        l.draw("│", style::separator());
        l.draw("  %  ", style::primary().bold());
        l.draw("│", style::separator());
        l.draw("histogram", style::primary().bold());

        for (i, (v, count, ty)) in rows.into_iter().enumerate() {
            let style = if i == self.nav.c_row {
                style::selected()
            } else {
                style::primary()
            };
            let mut l = c.top();
            l.draw(
                format_args!("{}", fmt.field(&ty, v, &stat, stat.budget())),
                style,
            );
            l.draw("│", style::separator());
            l.draw(format_args!("{count:>0$}", nb_budget), style);
            l.draw("│", style::separator());
            let percent = count as f64 * 100. / nb_item.max(1) as f64;
            l.draw(format_args!("{percent:>5.2}"), style);
            l.draw("│", style::separator());
            let nb_star = l.width().max(1) as u64 * count / max.max(1);
            for _ in 0..nb_star {
                l.draw("*", style);
            }
        }
    }
}
