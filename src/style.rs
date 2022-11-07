use tui::{none, Color, Style};

pub const fn error() -> Style {
    none().fg(Color::Red)
}

pub const fn primary() -> Style {
    none()
}

pub const fn progress() -> Style {
    none().fg(Color::Green)
}

pub const fn secondary() -> Style {
    none().fg(Color::DarkGrey)
}

pub const fn selected() -> Style {
    none().fg(Color::DarkYellow)
}

pub fn separator() -> Style {
    none().fg(Color::DarkGrey).dim()
}

pub fn state_action() -> Style {
    none().bg(Color::Green).bold()
}

pub fn state_alternate() -> Style {
    none().bg(Color::Magenta).bold()
}

pub fn state_default() -> Style {
    none().bg(Color::DarkGrey).bold()
}
