use crate::BrewResult;
use crate::delegate;
use crate::fetch as fetch_support;
use crate::global;
use std::collections::HashSet;
use std::process::ExitCode;

pub fn run(args: &[String]) -> BrewResult<ExitCode> {
    if args.len() < 2 {
        return delegate::run_with_reason(
            args,
            "fetch",
            "only simple named formula arguments are supported.",
        );
    }
    if args[1..].iter().any(|arg| arg.starts_with('-')) {
        return delegate::run_with_reason(args, "fetch", "flags are not yet supported.");
    }
    if args[1..]
        .iter()
        .any(|arg| !fetch_support::is_simple_formula_name(arg))
    {
        return delegate::run_with_reason(
            args,
            "fetch",
            "only simple named formula arguments are supported.",
        );
    }

    let api_cache = global::cache_api_path()?;
    let aliases = fetch_support::load_aliases(&api_cache.join("formula_aliases.txt"))?;
    let bottle_tag = fetch_support::current_bottle_tag()?;
    let client = fetch_support::build_client()?;
    let mut signed_cache_formulae = None;
    let mut bottles = Vec::with_capacity(args.len() - 1);
    let mut cached_downloads = HashSet::with_capacity(args.len() - 1);

    for name in &args[1..] {
        match fetch_support::resolve_bottle(
            name,
            &aliases,
            &api_cache,
            &mut signed_cache_formulae,
            &bottle_tag,
            &client,
        )? {
            fetch_support::Resolution::Bottle(resolved) => {
                let bottle = resolved.bottle.clone();
                if cached_downloads.insert(bottle.cached_download.clone()) {
                    bottles.push(bottle);
                }
            }
            fetch_support::Resolution::Delegate(reason) => {
                return delegate::run_with_reason(args, "fetch", &reason);
            }
        }
    }

    fetch_support::fetch_bottles(&bottles, &client)?;

    Ok(ExitCode::SUCCESS)
}
