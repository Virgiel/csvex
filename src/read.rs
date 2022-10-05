use std::{
    fs::File,
    io::{self, BufRead, BufReader, Seek, SeekFrom},
};

use bstr::{BStr, ByteSlice};
use csv_core::ReadRecordResult;

use crate::BUF_LEN;

use self::sniffing::{sniff_delimiter, sniff_has_header};

pub struct CsvReader {
    file: BufReader<File>,
    rdr: csv_core::Reader,
}

impl CsvReader {
    fn new(path: &str, delimiter: u8) -> io::Result<Self> {
        Ok(Self {
            file: BufReader::new(File::open(path)?),
            rdr: csv_core::ReaderBuilder::new().delimiter(delimiter).build(),
        })
    }

    /// Read a record into a nested string
    pub fn record(&mut self, nested: &mut NestedString) -> io::Result<()> {
        nested.read_record(&mut self.file, &mut self.rdr)
    }

    /// Read a record into a nested string from a random place in CSV file
    pub fn record_at(&mut self, nested: &mut NestedString, offset: u64) -> io::Result<()> {
        let pos = self.file.stream_position()?; // syscall without disk read
        self.file.seek_relative(offset as i64 - pos as i64)?; // keep buffer if close to current position
        self.rdr.reset();
        self.record(nested)
    }

    pub fn inner_mut(&mut self) -> (&mut BufReader<File>, &mut csv_core::Reader) {
        (&mut self.file, &mut self.rdr)
    }
}

pub struct NestedString {
    /// Common allocation for all strings
    buff: Vec<u8>,
    /// End of each string in buf
    bounds: Vec<usize>,
}

impl NestedString {
    pub fn new() -> Self {
        Self {
            buff: Vec::with_capacity(BUF_LEN),
            bounds: vec![0],
        }
    }

    fn read_record(
        &mut self,
        file: &mut BufReader<File>,
        rdr: &mut csv_core::Reader,
    ) -> io::Result<()> {
        self.clear();

        let mut nb_buff = 0;
        let mut nb_bounds = 1;

        self.buff.resize(self.buff.capacity(), 0); // No allocation
        self.bounds.resize(self.bounds.capacity(), 0); // No allocation

        loop {
            let buff = file.fill_buf()?;
            let (result, r_in, r_out, r_bound) = rdr.read_record(
                buff,
                &mut self.buff[nb_buff..],
                &mut self.bounds[nb_bounds..],
            );
            file.consume(r_in);
            nb_buff += r_out;
            nb_bounds += r_bound;

            match result {
                ReadRecordResult::InputEmpty => continue, // Will be filled in next iteration
                ReadRecordResult::OutputFull => self.buff.resize(self.buff.capacity() * 2, 0), // Double buffer len
                ReadRecordResult::OutputEndsFull => {
                    self.bounds.resize(self.bounds.capacity() * 2, 0)
                } // Double buffer len
                ReadRecordResult::Record | ReadRecordResult::End => break,
            }
        }
        self.buff.resize(nb_buff, 0);
        self.bounds.resize(nb_bounds, 0);
        Ok(())
    }

    /// Lazily trimmed and escaped str field

    fn get_str_trim<'a>(&'a self, start: usize, end: usize) -> &BStr {
        BStr::new(BStr::new(&self.buff[start..end]).trim())
    }

    /// Lazily trimmed and escaped str field
    pub fn get<'a>(&'a self, idx: usize) -> Option<&BStr> {
        (idx < self.len()).then(|| self.get_str_trim(self.bounds[idx], self.bounds[idx + 1]))
    }

    pub fn len(&self) -> usize {
        self.bounds.len() - 1
    }

    pub fn clear(&mut self) {
        self.buff.clear();
        self.bounds.resize(1, 0);
    }

    pub fn iter(&self) -> impl Iterator<Item = &BStr> {
        self.bounds.windows(2).map(|win| match win {
            [start, end] => self.get_str_trim(*start, *end),
            _ => unreachable!(),
        })
    }
}

#[derive(Clone)]
pub struct Config {
    pub path: String,
    pub delimiter: u8,
    pub has_header: bool,
}

impl Config {
    pub fn sniff(path: String) -> io::Result<(Self, CsvReader)> {
        let mut file = BufReader::new(File::open(&path)?);
        let delimiter = sniff_delimiter(&mut file)?;
        file.seek(SeekFrom::Start(0))?;
        let mut rdr = CsvReader {
            file,
            rdr: csv_core::ReaderBuilder::new().delimiter(delimiter).build(),
        };
        let has_header = sniff_has_header(&mut rdr)?;
        Ok((
            Self {
                path,
                delimiter,
                has_header,
            },
            rdr,
        ))
    }

    pub fn reader(&self) -> io::Result<CsvReader> {
        CsvReader::new(&self.path, self.delimiter)
    }
}

mod sniffing {
    use std::{
        borrow::Cow,
        fs::File,
        io::{self, BufRead, BufReader},
    };

    use bstr::ByteSlice;

    use crate::read::NestedString;

    use super::CsvReader;

    /// Guess the csv delimiter from the first line
    pub fn sniff_delimiter(file: &mut BufReader<File>) -> io::Result<u8> {
        const DELIMITER: [u8; 5] = [b',', b';', b':', b'|', b'_'];
        let mut counter = [0; DELIMITER.len()];

        'main: loop {
            let buff = file.fill_buf()?;
            if buff.is_empty() {
                break 'main;
            }
            for c in buff {
                if *c == b'\n' {
                    break 'main;
                }
                // Count occurrence of delimiter char
                if let Some((count, _)) = counter.iter_mut().zip(DELIMITER).find(|(_, d)| d == c) {
                    *count += 1;
                }
            }
            let amount = buff.len();
            file.consume(amount);
        }
        // Return most used delimiter or ',' by default
        Ok(counter
            .iter()
            .zip(DELIMITER)
            .max_by_key(|(c, _)| *c)
            .map(|(_, d)| d)
            .unwrap_or(DELIMITER[0]))
    }

    #[derive(PartialEq, Eq)]
    enum TY {
        String,
        Number,
        Bool,
    }

    fn sniff_ty<'a>(str: Cow<'a, str>) -> TY {
        if str.parse::<bool>().is_ok() {
            TY::Bool
        } else if str.parse::<f64>().is_ok() {
            TY::Number
        } else {
            TY::String
        }
    }

    /// Guess the csv delimiter from the first line
    pub fn sniff_has_header(rdr: &mut CsvReader) -> io::Result<bool> {
        let mut row = NestedString::new();
        rdr.record(&mut row)?; // Read headers
        let mut tys = Vec::with_capacity(row.len());
        for field in row.iter() {
            // header should not be empty
            if field.is_empty() {
                return Ok(false);
            }
            tys.push(sniff_ty(field.to_str_lossy()));
        }

        let all_str = tys.iter().all(|t| *t == TY::String);
        rdr.record(&mut row)?; // Read first supposed data
        let mut all_same = true;
        for (i, field) in row.iter().enumerate() {
            let ty = sniff_ty(field.to_str_lossy());
            // typical headers are all string and a data column are not
            if all_str && ty != TY::String {
                return Ok(true);
            }

            if ty != tys[i] {
                all_same = false;
            }
        }

        // typically data column have coherent type
        if all_same && !all_str {
            return Ok(false);
        }

        // default CSV commonly have headers
        Ok(true)
    }
}
