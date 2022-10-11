use std::{
    fs::File,
    io::{self, BufRead, BufReader, Seek, SeekFrom},
};

use crate::fmt::Ty;

use super::{CsvReader, StringRecord};

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
        let mut rdr = CsvReader::from_file(file, delimiter);
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

/// Guess the csv delimiter from the first line
fn sniff_delimiter(file: &mut BufReader<File>) -> io::Result<u8> {
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

/// Guess the csv delimiter from the first line
fn sniff_has_header(rdr: &mut CsvReader) -> io::Result<bool> {
    let mut record = StringRecord::new();
    rdr.record(&mut record)?; // Read headers
    let mut tys = Vec::with_capacity(record.len());
    let mut found_empty = false;
    for field in record.iter() {
        if field.is_empty() {
            if !found_empty {
                // Last row can be a fake one
                found_empty = true;
            } else {
                // header should not be empty
                return Ok(false);
            }
        }
        tys.push(Ty::guess(field));
    }

    let all_str = tys.iter().all(|b| b.is_str());
    rdr.record(&mut record)?; // Read first supposed data
    let mut all_same = true;
    for (i, field) in record.iter().enumerate() {
        let ty = Ty::guess(field);
        // typical headers are all string and a data column are not
        if all_str && !ty.is_str() {
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
    Ok(false)
}
