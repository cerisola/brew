use crate::BrewResult;
use crate::delegate;
use crate::fetch as fetch_support;
use crate::formula_installer::{self, InstallAction, InstallPlan};
use crate::global;
use std::process::ExitCode;

pub fn run(args: &[String]) -> BrewResult<ExitCode> {
    if args.len() != 2 {
        return delegate::run_with_reason(
            args,
            "install",
            "only a single simple named formula argument is supported.",
        );
    }
    if args[1].starts_with('-') {
        return delegate::run_with_reason(args, "install", "flags are not yet supported.");
    }
    if !fetch_support::is_simple_formula_name(&args[1]) {
        return delegate::run_with_reason(
            args,
            "install",
            "only a single simple named formula argument is supported.",
        );
    }

    let api_cache = global::cache_api_path()?;
    let aliases = fetch_support::load_aliases(&api_cache.join("formula_aliases.txt"))?;
    let bottle_tag = fetch_support::current_bottle_tag()?;
    let client = fetch_support::build_client()?;
    let mut signed_cache_formulae = None;

    let resolved = match fetch_support::resolve_bottle(
        &args[1],
        &aliases,
        &api_cache,
        &mut signed_cache_formulae,
        &bottle_tag,
        &client,
    )? {
        fetch_support::Resolution::Bottle(resolved) => resolved,
        fetch_support::Resolution::Delegate(reason) => {
            return delegate::run_with_reason(args, "install", &reason);
        }
    };

    let install_plan = match formula_installer::resolve_install_plan(
        *resolved,
        &aliases,
        &api_cache,
        &mut signed_cache_formulae,
        &bottle_tag,
        &client,
    )? {
        InstallPlan::Actions(actions) => actions,
        InstallPlan::Delegate(reason) => {
            return delegate::run_with_reason(args, "install", &reason);
        }
    };

    // TODO: Add argument parity for multi-formula installs, local formula paths, taps, and flags.
    // TODO: Add `FormulaInstaller#check_install_sanity`, locking, and conflict checks before mutating the Cellar.
    // TODO: Add relocation and dynamic linkage handling for bottles that are not `:any_skip_relocation`.
    // TODO: Add `post_install`, `Tab` writes, SBOM writes, services, caveats, and global post-install hooks.
    // TODO: Replace the temporary Ruby `brew link` reuse with Rust parity for `Keg#link`.
    // TODO: Validate bottle archive entries before extraction instead of trusting `tar` to keep paths contained.

    fetch_support::fetch_bottles(
        &install_plan
            .iter()
            .filter_map(|action| match action {
                InstallAction::Pour(resolved) => Some(resolved.bottle.clone()),
                InstallAction::Link(_) => None,
            })
            .collect::<Vec<_>>(),
        &client,
    )?;

    for action in install_plan {
        let exit_code = match action {
            InstallAction::Link(formula_name) => {
                formula_installer::link_installed_keg(&formula_name)?
            }
            InstallAction::Pour(resolved) => {
                formula_installer::pour_bottle(&resolved)?;
                formula_installer::link_installed_keg(&resolved.formula.name)?
            }
        };
        if exit_code != ExitCode::SUCCESS {
            return Ok(exit_code);
        }
    }

    Ok(ExitCode::SUCCESS)
}
