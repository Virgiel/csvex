use std::{
    borrow::Cow,
    fs::File,
    io::{self, BufRead, BufReader, Seek, SeekFrom},
    path::PathBuf,
    time::{Duration, Instant, SystemTime},
};

use crate::{
    fmt::Ty,
    reader::{CsvReader, NestedString},
};

pub const WATCHER_POOL: Duration = Duration::from_secs(1);

enum SourceKind {
    File {
        path: PathBuf,
        last: Instant,
        m_time: SystemTime,
    },
    Stdin {
        tmp: tempfile::NamedTempFile,
    },
}

impl SourceKind {
    pub fn path(&self) -> Cow<str> {
        match &self {
            SourceKind::File { path, .. } => path.to_string_lossy(),
            SourceKind::Stdin { .. } => "stdin".into(),
        }
    }

    pub fn open(&self) -> io::Result<File> {
        match &self {
            SourceKind::File { path, .. } => std::fs::File::open(path),
            SourceKind::Stdin { tmp } => std::fs::File::open(tmp.path()),
        }
    }
}

pub struct Source {
    kind: SourceKind,
    pub delimiter: u8,
    pub has_header: bool,
    pub display_path: String,
}

impl Source {
    pub fn new(filename: Option<PathBuf>) -> io::Result<(Self, CsvReader)> {
        let kind = if let Some(path) = filename {
            let m_time = std::fs::metadata(&path)?.modified()?;
            SourceKind::File {
                path,
                last: Instant::now(),
                m_time,
            }
        } else {
            let mut stdin = std::io::stdin();
            let mut tmp = tempfile::NamedTempFile::new()?;
            std::io::copy(&mut stdin, &mut tmp)?;
            SourceKind::Stdin { tmp }
        };
        let display_path = kind.path().to_string();
        let mut file = BufReader::new(kind.open()?);
        let delimiter = sniff_delimiter(&mut file)?;
        file.seek(SeekFrom::Start(0))?;
        let mut rdr = CsvReader::new(file, delimiter);
        let has_header = sniff_has_header(&mut rdr)?;
        Ok((
            Self {
                kind,
                delimiter,
                has_header,
                display_path,
            },
            rdr,
        ))
    }

    pub fn refresh(&mut self) -> io::Result<CsvReader> {
        let mut file = BufReader::new(self.kind.open()?);
        self.delimiter = sniff_delimiter(&mut file)?;
        file.seek(SeekFrom::Start(0))?;
        let mut rdr = CsvReader::new(file, self.delimiter);
        self.has_header = sniff_has_header(&mut rdr)?;
        Ok(rdr)
    }

    pub fn reader(&self) -> io::Result<CsvReader> {
        Ok(CsvReader::new(
            BufReader::new(self.kind.open()?),
            self.delimiter,
        ))
    }

    pub fn check_dirty(&mut self) -> std::io::Result<bool> {
        Ok(match &mut self.kind {
            SourceKind::File { path, last, m_time } => {
                if last.elapsed() < WATCHER_POOL {
                    false
                } else {
                    *last = Instant::now();
                    let new_m_time = std::fs::metadata(&path)?.modified()?;
                    if new_m_time != *m_time {
                        *m_time = new_m_time;
                        true
                    } else {
                        false
                    }
                }
            }
            SourceKind::Stdin { .. } => false,
        })
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
    let mut record = NestedString::new();
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
