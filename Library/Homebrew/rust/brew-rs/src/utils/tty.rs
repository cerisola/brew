use crate::global;
use std::fmt;
use std::io::{self, IsTerminal};
use std::process::Command;
use std::str;
use std::sync::OnceLock;

// Color codes
const RED: &str = "31";
const GREEN: &str = "32";
const YELLOW: &str = "33";
const BLUE: &str = "34";

// Style codes
const RESET: &str = "0";
const BOLD: &str = "1";
const UNDERLINE: &str = "4";

static TTY_WIDTH: OnceLock<usize> = OnceLock::new();

pub struct AnsiBuilder {
    escape_sequences: Vec<&'static str>,
}

fn colorful_output() -> bool {
    if global::brew_no_color() {
        return false;
    }
    if global::brew_color() {
        return true;
    }
    io::stdout().is_terminal()
}

impl fmt::Display for AnsiBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if colorful_output() {
            write!(f, "\x1B[{}m", self.escape_sequences.join(";"))
        } else {
            write!(f, "")
        }
    }
}

impl AnsiBuilder {
    pub fn new() -> Self {
        AnsiBuilder {
            escape_sequences: Vec::new(),
        }
    }
}

macro_rules! ansi_methods_and_functions {
    ($( $name:ident => $value:expr ),* $(,)?) => {
        impl AnsiBuilder {
            $(
                #[inline]
                pub fn $name(mut self) -> Self {
                    self.escape_sequences.push($value);
                    self
                }
            )*
        }

        $(
            #[inline]
            #[allow(unused)]
            pub fn $name() -> AnsiBuilder {
                AnsiBuilder::new().$name()
            }
        )*
    };
}

ansi_methods_and_functions! {
    red => RED,
    green => GREEN,
    yellow => YELLOW,
    blue => BLUE,
    reset => RESET,
    bold => BOLD,
    underline => UNDERLINE,
}

pub struct MoveCursorUp(pub usize);

impl fmt::Display for MoveCursorUp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\x1B[{}A", self.0)
    }
}

#[inline]
pub fn move_cursor_up(line_count: usize) -> MoveCursorUp {
    MoveCursorUp(line_count)
}

#[inline]
pub fn clear_to_end() -> &'static str {
    "\x1B[K"
}

#[inline]
pub fn clear_entire_line() -> &'static str {
    "\x1B[2K"
}

#[inline]
pub fn hide_cursor() -> &'static str {
    "\x1B[?25l"
}

#[inline]
pub fn show_cursor() -> &'static str {
    "\x1B[?25h"
}
pub fn width() -> usize {
    *TTY_WIDTH.get_or_init(|| {
        if let Ok(command) = Command::new("/bin/stty").arg("size").output() {
            let mut output = command.stdout.split(|ch| ch.is_ascii_whitespace());

            let _ = output.next();
            if let Some(width) = output
                .next()
                .and_then(|string| str::from_utf8(string).ok())
                .and_then(|string| string.parse().ok())
            {
                return width;
            }
        }

        if let Ok(output) = Command::new("/usr/bin/tput").arg("cols").output()
            && let Some(res) = str::from_utf8(&output.stdout)
                .ok()
                .and_then(|string| string.parse().ok())
        {
            return res;
        }

        80
    })
}

pub fn truncate(string: &str) -> &str {
    let w = width();

    if w < 4 {
        return string;
    }

    let mut end = usize::min(string.len(), w - 4);

    while end > 0 && !string.is_char_boundary(end) {
        end -= 1;
    }

    &string[..end]
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_ansi_builder() {
        let expected = if colorful_output() {
            "\x1B[31;1;4m"
        } else {
            ""
        };
        assert_eq!(
            expected,
            AnsiBuilder::new()
                .red()
                .bold()
                .underline()
                .to_string()
                .as_str()
        );
    }

    #[test]
    fn test_truncate() {
        let w = width();

        if w < 4 {
            let string = "some string that is longer than 4 characters for sure";
            assert_eq!(string.len(), truncate(string).len());
            return;
        }

        let not_trimmed = "a".repeat(w - 4);
        assert_eq!(not_trimmed.len(), truncate(&not_trimmed).len());
        let trimmed = "a".repeat(w);
        assert_eq!(trimmed.len() - 4, truncate(&trimmed).len());
    }
}
