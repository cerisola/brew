#![forbid(unsafe_code)]

mod brew;
mod cmd;
mod delegate;
mod download_queue;
mod fetch;
mod formula_installer;
mod global;
mod search;
mod utils;

use anyhow::Result;
use std::process::ExitCode;

pub fn main() -> ExitCode {
    brew::main()
}

type BrewResult<T> = Result<T>;
