use std::{
    io,
    time::{Duration, Instant, SystemTime},
};

pub const WATCHER_POOL: Duration = Duration::from_secs(1);

pub struct FileWatcher {
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
