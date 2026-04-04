use crate::utils::tty::{self, AnsiBuilder};
use std::io::{self, IsTerminal};

pub fn ohai(string: &str) {
    let title = if io::stdout().is_terminal() {
        tty::truncate(string)
    } else {
        string
    };
    println!("{}", headline(title, &tty::blue()));
}

pub fn warning(string: &str) {
    eprintln!(
        "{warning}Warning{reset}: {string}",
        warning = AnsiBuilder::new().yellow().underline(),
        reset = tty::reset(),
    );
}

pub fn error(string: &str) {
    eprintln!(
        "{error}Error{reset}: {string}",
        error = tty::red(),
        reset = tty::reset(),
    );
}

fn arrow(string: &str, escape_sequence: &AnsiBuilder) -> String {
    prefix("==>", string, escape_sequence)
}

fn headline(string: &str, escape_sequence: &AnsiBuilder) -> String {
    arrow(
        format!(
            "{bold}{string}{reset}",
            bold = tty::bold(),
            reset = tty::reset(),
        )
        .as_str(),
        escape_sequence,
    )
}

fn prefix(prefix: &str, string: &str, escape_sequence: &AnsiBuilder) -> String {
    if prefix.trim().is_empty() {
        return format!("{escape_sequence}{string}{reset}", reset = tty::reset());
    }

    format!(
        "{escape_sequence}{prefix}{reset} {string}",
        reset = tty::reset()
    )
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_prefix() {
        let empty_prefix = "     ";
        let non_empty_prefix = "==>";
        let escape_sequence = AnsiBuilder::new().yellow().bold();
        assert_eq!(
            format!(
                "{custom_color}test{reset}",
                custom_color = &escape_sequence,
                reset = tty::reset()
            ),
            prefix(empty_prefix, "test", &escape_sequence)
        );
        assert_eq!(
            format!(
                "{custom_color}{non_empty_prefix}{reset} test",
                custom_color = &escape_sequence,
                reset = tty::reset()
            ),
            prefix(non_empty_prefix, "test", &escape_sequence)
        );
    }

    #[test]
    fn test_headline() {
        let test = "1234foobar";
        let escape_sequence = AnsiBuilder::new().underline().red();

        assert_eq!(
            format!(
                "{color}==>{reset} {bold}{test}{reset}",
                color = &escape_sequence,
                bold = tty::bold(),
                reset = tty::reset(),
            ),
            headline(test, &escape_sequence)
        );
    }
}
