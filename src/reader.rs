use std::{
    fs::File,
    io::{self, BufRead, BufReader, Seek},
    ops::{Deref, DerefMut, Range},
};

use bstr::{BStr, ByteSlice};
use csv_core::ReadRecordResult;

use crate::BUF_LEN;

pub struct CsvReader {
    file: BufReader<File>,
    rdr: csv_core::Reader,
}

impl CsvReader {
    pub(crate) fn new(file: BufReader<File>, delimiter: u8) -> Self {
        Self {
            file,
            rdr: csv_core::ReaderBuilder::new().delimiter(delimiter).build(),
        }
    }

    /// Read a record into a nested string
    pub fn record(&mut self, nested: &mut NestedString) -> io::Result<usize> {
        nested.read_record(&mut self.file, &mut self.rdr)
    }

    /// Read a record into a nested string from a random place in CSV file
    pub fn record_at(&mut self, nested: &mut NestedString, offset: u64) -> io::Result<usize> {
        self.seek(offset)?;
        self.record(nested)
    }

    pub fn seek(&mut self, offset: u64) -> io::Result<()> {
        let pos = self.file.stream_position()?; // syscall without disk read
        self.file.seek_relative(offset as i64 - pos as i64)?; // keep buffer if close to current position
        self.rdr.reset();
        Ok(())
    }

    pub fn pos(&mut self) -> io::Result<u64> {
        self.file.stream_position()
    }

    pub fn len(&self) -> io::Result<u64> {
        Ok(self.file.get_ref().metadata()?.len())
    }
}

/// Byte vector that is backed by an always initialize slice so we can write in the currently
/// unused space whiteout UB or expensive zeroing
pub struct InitVec<T: Default + Copy, const N: usize> {
    buff: Box<[T]>,
    len: usize,
}

impl<T: Default + Copy, const N: usize> InitVec<T, N> {
    /// Init the vector with N unused elements initialized as T::default()
    pub fn new() -> Self {
        Self {
            buff: vec![T::default(); N].into_boxed_slice(),
            len: 0,
        }
    }

    /// Get the unused elements of the vector
    pub fn unused(&mut self) -> &mut [T] {
        &mut self.buff[self.len..]
    }

    /// Augment the number of used elements
    pub fn advance(&mut self, amount: usize) {
        self.set_len(self.len.saturating_add(amount));
    }

    /// Set the number of used elements
    pub fn set_len(&mut self, len: usize) {
        while len > self.buff.len() {
            self.grow();
        }
        self.len = len;
    }

    /// Augment capacity by N
    pub fn grow(&mut self) {
        let mut vec = std::mem::take(&mut self.buff).into_vec();
        vec.resize(vec.len() + N, T::default());
        self.buff = vec.into_boxed_slice();
    }
}

impl<T: Default + Copy, const N: usize> Deref for InitVec<T, N> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.buff[..self.len]
    }
}

impl<T: Default + Copy, const N: usize> DerefMut for InitVec<T, N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.buff[..self.len]
    }
}

pub struct NestedString {
    /// Common allocation for all strings
    buff: InitVec<u8, BUF_LEN>,
    /// End of each string in buf
    bounds: InitVec<usize, 50>,
}

impl NestedString {
    pub fn new() -> Self {
        let mut bounds = InitVec::new();
        bounds.set_len(1);
        Self {
            buff: InitVec::new(),
            bounds,
        }
    }

    fn read_record(
        &mut self,
        file: &mut BufReader<File>,
        rdr: &mut csv_core::Reader,
    ) -> io::Result<usize> {
        // Reset buffer
        self.buff.set_len(0);
        self.bounds.set_len(1);

        let mut nb_read = 0;

        // Read record
        loop {
            let buff = file.fill_buf()?;
            let (result, r_in, r_out, r_bound) =
                rdr.read_record(buff, self.buff.unused(), self.bounds.unused());
            file.consume(r_in);
            nb_read += r_in;
            self.buff.advance(r_out);
            self.bounds.advance(r_bound);

            match result {
                ReadRecordResult::InputEmpty => continue,
                ReadRecordResult::OutputFull => self.buff.grow(),
                ReadRecordResult::OutputEndsFull => self.bounds.grow(),
                ReadRecordResult::Record | ReadRecordResult::End => break,
            }
        }
        // Collapse empty column a the end
        if self.bounds.len() > 2
            && self.bounds[self.bounds.len() - 1] == self.bounds[self.bounds.len() - 2]
        {
            self.bounds.set_len(self.bounds.len() - 1)
        }
        Ok(nb_read)
    }

    fn get_range(&self, range: Range<usize>) -> &BStr {
        BStr::new(BStr::new(&self.buff[range]).trim())
    }

    pub fn get(&self, idx: usize) -> Option<&BStr> {
        (idx < self.len()).then(|| self.get_range(self.bounds[idx]..self.bounds[idx + 1]))
    }

    pub fn len(&self) -> usize {
        self.bounds.len() - 1
    }

    pub fn iter(&self) -> impl Iterator<Item = &BStr> {
        self.bounds.windows(2).map(|win| match win {
            [start, end] => self.get_range(*start..*end),
            _ => unreachable!(),
        })
    }
}
