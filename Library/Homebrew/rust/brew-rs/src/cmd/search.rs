use crate::BrewResult;
use crate::delegate;
use crate::global;
use crate::search;
use std::process::ExitCode;

pub fn run(args: &[String]) -> BrewResult<ExitCode> {
    if args.len() != 2 || args[1].starts_with('-') {
        return delegate::run(args);
    }

    let api_cache = global::cache_api_path()?;
    let formula_names = match global::read_lines(&api_cache.join("formula_names.txt")) {
        Ok(names) if !names.is_empty() => names,
        _ => return delegate::run(args),
    };
    let cask_names = match global::read_lines(&api_cache.join("cask_names.txt")) {
        Ok(names) => names,
        Err(_) => return delegate::run(args),
    };

    let formulae = search::search_names(&formula_names, &args[1])?;
    let casks = search::search_names(&cask_names, &args[1])?;

    if formulae.is_empty() && casks.is_empty() {
        eprintln!("No formulae or casks found for {:?}.", args[1]);
        return Ok(ExitCode::FAILURE);
    }

    global::print_sections(&formulae, &casks);
    Ok(ExitCode::SUCCESS)
}
