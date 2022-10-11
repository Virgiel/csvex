use std::{
    fs::File,
    io::{self, BufRead, BufReader, Seek},
    ops::{Deref, DerefMut, Range},
};

use bstr::{BStr, ByteSlice};
use csv_core::ReadRecordResult;

use crate::BUF_LEN;

pub struct CsvReader {
    pub file: BufReader<File>,
    pub rdr: csv_core::Reader,
}

impl CsvReader {
    pub(crate) fn from_file(file: BufReader<File>, delimiter: u8) -> Self {
        Self {
            file,
            rdr: csv_core::ReaderBuilder::new().delimiter(delimiter).build(),
        }
    }

    pub(crate) fn new(path: &str, delimiter: u8) -> io::Result<Self> {
        Ok(Self::from_file(
            BufReader::new(File::open(path)?),
            delimiter,
        ))
    }

    /// Read a record into a nested string
    pub fn record(&mut self, nested: &mut impl Record) -> io::Result<usize> {
        nested.read_record(&mut self.file, &mut self.rdr)
    }

    /// Read a record into a nested string from a random place in CSV file
    pub fn record_at(&mut self, nested: &mut impl Record, offset: u64) -> io::Result<usize> {
        self.seek(offset)?;
        self.record(nested)
    }

    pub fn seek(&mut self, offset: u64) -> io::Result<()> {
        let pos = self.file.stream_position()?; // syscall without disk read
        self.file.seek_relative(offset as i64 - pos as i64)?; // keep buffer if close to current position
        self.rdr.reset();
        Ok(())
    }

    pub fn inner_mut(&mut self) -> (&mut BufReader<File>, &mut csv_core::Reader) {
        (&mut self.file, &mut self.rdr)
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

pub trait Record {
    fn read_record(
        &mut self,
        file: &mut BufReader<File>,
        rdr: &mut csv_core::Reader,
    ) -> io::Result<usize>;
}

pub struct StringRecord(BytesRecord);

impl Record for StringRecord {
    fn read_record(
        &mut self,
        file: &mut BufReader<File>,
        rdr: &mut csv_core::Reader,
    ) -> io::Result<usize> {
        let amount = self.0.read_record(file, rdr)?;
        self.0.in_place_str_lossy();
        Ok(amount)
    }
}

impl StringRecord {
    pub fn new() -> Self {
        Self(BytesRecord::new())
    }

    pub fn get(&self, idx: usize) -> Option<&str> {
        self.0.get(idx).map(|it| {
            debug_assert!(std::str::from_utf8(it).is_ok());
            unsafe { std::str::from_utf8_unchecked(it) }
        })
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|it| {
            debug_assert!(std::str::from_utf8(it).is_ok());
            unsafe { std::str::from_utf8_unchecked(it) }
        })
    }
}

pub struct BytesRecord {
    /// Common allocation for all strings
    buff: InitVec<u8, BUF_LEN>,
    /// End of each string in buf
    bounds: InitVec<usize, 50>,
}

impl Record for BytesRecord {
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
        Ok(nb_read)
    }
}

impl BytesRecord {
    pub fn new() -> Self {
        let mut bounds = InitVec::new();
        bounds.set_len(0);
        Self {
            buff: InitVec::new(),
            bounds,
        }
    }

    /// Clean fields, turn into trimmed valid UTF-8 fields in place
    ///
    /// It's very expensive, you don't want to not run it if you don't need a valid utf8,
    /// but if you do need it, you want it to be cached and allocation free
    fn in_place_str_lossy(&mut self) {
        let mut write_pos = 0;
        let mut prev = 0;
        let mut read_offset = 0;
        for i in 0..self.bounds.len() - 1 {
            let (mut start, mut end) = (prev + read_offset, self.bounds[i + 1] + read_offset);
            // Trim
            start = end - BStr::new(&self.buff[start..end]).trim_start().len();
            end = start + BStr::new(&self.buff[start..end]).trim_end().len();

            // In place to_str
            while start != end {
                let (valid_up_to, error_len) = match BStr::new(&self.buff[start..end]).to_str() {
                    Ok(str) => (str.len(), 0),
                    Err(err) => (err.valid_up_to(), err.error_len().unwrap_or(0)),
                };
                // Move valid part
                if start != write_pos {
                    self.buff.copy_within(start..start + valid_up_to, write_pos)
                }
                write_pos += valid_up_to;
                start += valid_up_to;
                // Replace error part
                if error_len != 0 {
                    if start - write_pos < 3 {
                        let from = start + error_len;
                        let amount = self.buff.len() - from;
                        self.buff.advance(3);
                        self.buff.copy_within(from..from + amount, from + 3);
                        read_offset += 3;
                        start += 3;
                        end += 3;
                    }
                    self.buff[write_pos..][..3].copy_from_slice(&[239, 191, 189]);
                    write_pos += 3;
                    start += error_len;
                }
            }
            prev = self.bounds[i + 1];
            self.bounds[i + 1] = write_pos;
        }
    }

    fn get_range(&self, range: Range<usize>) -> &BStr {
        BStr::new(&self.buff[range])
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
