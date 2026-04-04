use crate::BrewResult;
use crate::cmd;
use std::process::ExitCode;

pub fn run(args: &[String]) -> BrewResult<ExitCode> {
    cmd::run_with_warning(args, "uninstall")
}
