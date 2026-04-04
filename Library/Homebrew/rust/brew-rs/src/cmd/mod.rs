use crate::BrewResult;
use crate::delegate;
use std::process::ExitCode;

pub mod fetch;
pub mod info;
pub mod install;
pub mod list;
pub mod reinstall;
pub mod search;
pub mod uninstall;
pub mod update;
pub mod upgrade;

pub(crate) fn run_with_warning(args: &[String], command_name: &str) -> BrewResult<ExitCode> {
    delegate::run_with_warning(args, command_name)
}
