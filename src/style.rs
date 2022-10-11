use tui::{none, Color, Style};

pub fn grey() -> Style {
    none().fg(Color::DarkGrey)
}

pub fn reverse(style: Style) -> Style {
    style
        .fg(style.bg.unwrap_or(Color::Black))
        .bg(style.fg.unwrap_or(Color::White))
}
