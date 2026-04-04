use crate::BrewResult;
use crate::cmd;
use crate::delegate;
use crate::utils::formatter;
use std::env;
use std::process::ExitCode;

pub(crate) fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            formatter::error(&error.to_string());
            ExitCode::FAILURE
        }
    }
}

fn run() -> BrewResult<ExitCode> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        return delegate::run(&args);
    }

    match args[0].as_str() {
        "fetch" => cmd::fetch::run(&args),
        "search" => cmd::search::run(&args),
        "info" => cmd::info::run(&args),
        "list" => cmd::list::run(&args),
        "install" => cmd::install::run(&args),
        "reinstall" => cmd::reinstall::run(&args),
        "update" => cmd::update::run(&args),
        "upgrade" => cmd::upgrade::run(&args),
        "uninstall" => cmd::uninstall::run(&args),
        _ => delegate::run(&args),
    }
}
