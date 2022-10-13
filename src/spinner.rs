use std::time::{Duration, Instant};

const SPINNER_TICK: Duration = Duration::from_millis(250);
const SPINNERS_UNICODE: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub struct Spinner {
    start: Option<Instant>,
}

impl Spinner {
    pub fn new() -> Self {
        Self { start: None }
    }

    pub fn state(&mut self, is_loading: bool) -> Option<char> {
        if is_loading && self.start.is_none() {
            self.start.replace(Instant::now());
        } else if !is_loading && self.start.is_some() {
            self.start.take();
        }
        if let Some(start) = self.start {
            let elapsed = start.elapsed();
            // Wait for some frame to prevent flashing
            if elapsed > SPINNER_TICK {
                let tick =
                    ((elapsed - SPINNER_TICK).as_millis() / 250) as usize % SPINNERS_UNICODE.len();
                return Some(SPINNERS_UNICODE[tick]);
            }
        }
        None
    }
}
