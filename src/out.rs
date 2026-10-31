//! Console output: `Write-Host` lines with a foreground colour, and raw frame writes.
#![expect(clippy::print_stdout, clippy::print_stderr, reason = "the CLI output module")]

use std::io::{IsTerminal as _, Write as _};

/// The `Write-Host -ForegroundColor` names the launcher uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fg {
    Plain,
    Red,
    DarkYellow,
    DarkGray,
    Cyan,
    Green,
    Magenta,
    Blue,
    Yellow,
}

impl Fg {
    /// A config tint name (`Green`, `Magenta`, ...) as a console colour.
    pub fn from_tint(name: &str) -> Self {
        match name {
            "Red" => Self::Red,
            "Cyan" => Self::Cyan,
            "Magenta" => Self::Magenta,
            "Blue" => Self::Blue,
            "Yellow" => Self::Yellow,
            _ => Self::Green,
        }
    }

    fn sgr(self) -> &'static str {
        match self {
            Self::Plain => "",
            Self::Red => "\x1b[91m",
            Self::DarkYellow => "\x1b[33m",
            Self::DarkGray => "\x1b[90m",
            Self::Cyan => "\x1b[96m",
            Self::Green => "\x1b[92m",
            Self::Magenta => "\x1b[95m",
            Self::Blue => "\x1b[94m",
            Self::Yellow => "\x1b[93m",
        }
    }
}

fn paint(text: &str, fg: Fg) -> String {
    // Write-Host colours the console, never a redirect: the preview's captured text stays plain.
    if fg == Fg::Plain || !std::io::stdout().is_terminal() {
        text.to_owned()
    } else {
        format!("{}{text}\x1b[0m", fg.sgr())
    }
}

/// `Write-Host <text> -ForegroundColor <fg>`.
pub fn line(text: &str, fg: Fg) {
    println!("{}", paint(text, fg));
}

/// `Write-Host -NoNewline`.
pub fn inline(text: &str, fg: Fg) {
    print!("{}", paint(text, fg));
    flush();
}

/// A raw write (escape sequences, a whole frame).
pub fn raw(text: &str) {
    let mut o = std::io::stdout().lock();
    if o.write_all(text.as_bytes()).and_then(|()| o.flush()).is_err() {
        // A console that refuses a frame has nothing better to be told.
    }
}

pub fn flush() {
    if std::io::stdout().flush().is_err() {
        // Same as raw: nothing to report to.
    }
}

/// `[Console]::Error.WriteLine`.
pub fn err_line(text: &str) {
    eprintln!("{text}");
}
