use crate::BrewResult;
use crate::global;
use crate::utils::formatter;
use anyhow::Context;
use std::process::ExitCode;
use std::process::{Command, Stdio};

pub(crate) fn run(args: &[String]) -> BrewResult<ExitCode> {
    run_command(args)
}

pub(crate) fn run_with_warning(args: &[String], command_name: &str) -> BrewResult<ExitCode> {
    formatter::warning(
        format!("brew-rs is handing {command_name} back to the Ruby backend.").as_str(),
    );
    run_command(args)
}

pub(crate) fn run_with_reason(
    args: &[String],
    command_name: &str,
    reason: &str,
) -> BrewResult<ExitCode> {
    eprintln!("Warning: brew-rs is handing {command_name} back to the Ruby backend: {reason}");
    run_command(args)
}

fn run_command(args: &[String]) -> BrewResult<ExitCode> {
    let status = Command::new(global::brew_file()?)
        .args(args)
        .env_remove("HOMEBREW_EXPERIMENTAL_RUST_FRONTEND")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("Failed to delegate to brew")?;

    Ok(ExitCode::from(
        status.code().unwrap_or(1).clamp(0, 255) as u8
    ))
}
