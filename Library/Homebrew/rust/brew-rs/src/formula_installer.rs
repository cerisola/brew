use crate::BrewResult;
use crate::fetch::{self, BottleFetch, FormulaJson, Resolution, ResolvedBottle};
use crate::global;
use crate::utils::formatter;
use anyhow::{Context, anyhow, bail};
use reqwest::blocking::Client;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) enum InstallPlan {
    Actions(Vec<InstallAction>),
    Delegate(String),
}

pub(crate) enum InstallAction {
    Link(String),
    Pour(Box<ResolvedBottle>),
}

struct InstallState {
    exact_prefix_exists: bool,
    linked_state_exists: bool,
    rack_exists: bool,
}

struct InstallPlanner<'a> {
    aliases: &'a HashMap<String, String>,
    api_cache: &'a Path,
    signed_cache_formulae: &'a mut Option<HashMap<String, FormulaJson>>,
    bottle_tag: &'a str,
    client: &'a Client,
    visiting: HashSet<String>,
    planned: HashSet<String>,
    actions: Vec<InstallAction>,
}

impl<'a> InstallPlanner<'a> {
    fn new(
        aliases: &'a HashMap<String, String>,
        api_cache: &'a Path,
        signed_cache_formulae: &'a mut Option<HashMap<String, FormulaJson>>,
        bottle_tag: &'a str,
        client: &'a Client,
    ) -> Self {
        Self {
            aliases,
            api_cache,
            signed_cache_formulae,
            bottle_tag,
            client,
            visiting: HashSet::new(),
            planned: HashSet::new(),
            actions: Vec::new(),
        }
    }

    fn resolve(mut self, resolved: ResolvedBottle) -> BrewResult<InstallPlan> {
        match self.append_install_actions(resolved, true)? {
            Some(reason) => Ok(InstallPlan::Delegate(reason)),
            None => Ok(InstallPlan::Actions(self.actions)),
        }
    }

    fn append_install_actions(
        &mut self,
        resolved: ResolvedBottle,
        root_formula: bool,
    ) -> BrewResult<Option<String>> {
        if self.planned.contains(&resolved.formula.full_name) {
            return Ok(None);
        }
        if !self.visiting.insert(resolved.formula.full_name.clone()) {
            return Ok(Some(format!(
                "`{}` has cyclic dependencies.",
                resolved.formula.full_name
            )));
        }

        let install_state = install_state(&resolved.formula)?;
        if !root_formula && install_state.exact_prefix_exists {
            if !install_state.linked_state_exists && resolved.formula.keg_only_reason.is_none() {
                self.actions
                    .push(InstallAction::Link(resolved.formula.name.clone()));
            }
            self.planned.insert(resolved.formula.full_name.clone());
            self.visiting.remove(&resolved.formula.full_name);
            return Ok(None);
        }

        if let Some(reason) = basic_install_delegate_reason(&resolved)? {
            self.visiting.remove(&resolved.formula.full_name);
            return Ok(Some(reason));
        }

        for dependency in &resolved.formula.dependencies {
            let dependency_name = dependency.as_str().ok_or_else(|| {
                anyhow!("Missing dependency name for {}", resolved.formula.full_name)
            })?;
            let dependency = match fetch::resolve_bottle(
                dependency_name,
                self.aliases,
                self.api_cache,
                self.signed_cache_formulae,
                self.bottle_tag,
                self.client,
            )? {
                Resolution::Bottle(resolved) => resolved,
                Resolution::Delegate(reason) => {
                    self.visiting.remove(&resolved.formula.full_name);
                    return Ok(Some(reason));
                }
            };
            if let Some(reason) = self.append_install_actions(*dependency, false)? {
                self.visiting.remove(&resolved.formula.full_name);
                return Ok(Some(reason));
            }
        }

        self.planned.insert(resolved.formula.full_name.clone());
        self.visiting.remove(&resolved.formula.full_name);
        self.actions.push(InstallAction::Pour(Box::new(resolved)));

        Ok(None)
    }
}

pub(crate) fn resolve_install_plan(
    resolved: ResolvedBottle,
    aliases: &HashMap<String, String>,
    api_cache: &Path,
    signed_cache_formulae: &mut Option<HashMap<String, FormulaJson>>,
    bottle_tag: &str,
    client: &Client,
) -> BrewResult<InstallPlan> {
    InstallPlanner::new(
        aliases,
        api_cache,
        signed_cache_formulae,
        bottle_tag,
        client,
    )
    .resolve(resolved)
}

fn install_state(formula: &FormulaJson) -> BrewResult<InstallState> {
    let prefix = global::prefix_path()?;

    Ok(InstallState {
        exact_prefix_exists: installed_prefix(formula)?.exists(),
        linked_state_exists: path_exists_or_is_symlink(&prefix.join("opt").join(&formula.name))?
            || path_exists_or_is_symlink(&prefix.join("var/homebrew/linked").join(&formula.name))?,
        rack_exists: global::cellar_path()?.join(&formula.name).exists(),
    })
}

fn basic_install_delegate_reason(resolved: &ResolvedBottle) -> BrewResult<Option<String>> {
    let formula = &resolved.formula;

    if formula.dependencies.iter().any(|dependency| {
        !dependency
            .as_str()
            .is_some_and(fetch::is_simple_formula_name)
    }) || !formula.build_dependencies.is_empty()
        || !formula.test_dependencies.is_empty()
        || !formula.recommended_dependencies.is_empty()
        || !formula.optional_dependencies.is_empty()
        || !formula.uses_from_macos.is_empty()
    {
        return Ok(Some(format!(
            "`{}` has unsupported dependency metadata.",
            formula.full_name
        )));
    }

    match formula.post_install_defined {
        Some(false) => {}
        Some(true) => {
            return Ok(Some(format!(
                "`{}` defines `post_install`.",
                formula.full_name
            )));
        }
        None => {
            return Ok(Some(format!(
                "`{}` is missing install metadata for `post_install`.",
                formula.full_name
            )));
        }
    }

    if !matches!(
        resolved.bottle.bottle_cellar.as_deref(),
        Some("any_skip_relocation" | ":any_skip_relocation")
    ) {
        return Ok(Some(format!(
            "`{}` requires bottle relocation support.",
            formula.full_name
        )));
    }

    if formula.keg_only_reason.is_some() {
        return Ok(Some(format!("`{}` is keg-only.", formula.full_name)));
    }

    if formula.service.is_some() {
        return Ok(Some(format!(
            "`{}` defines service files.",
            formula.full_name
        )));
    }

    if formula
        .caveats
        .as_deref()
        .is_some_and(|caveats| !caveats.trim().is_empty())
    {
        return Ok(Some(format!("`{}` defines caveats.", formula.full_name)));
    }

    let install_state = install_state(formula)?;
    if install_state.rack_exists {
        return Ok(Some(format!(
            "reinstalls and upgrades for `{}` are not yet supported.",
            formula.full_name
        )));
    }

    if install_state.linked_state_exists {
        return Ok(Some(format!(
            "`{}` already has linked install state.",
            formula.full_name
        )));
    }

    Ok(None)
}

fn path_exists_or_is_symlink(path: &Path) -> BrewResult<bool> {
    Ok(path.exists()
        || fs::symlink_metadata(path)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false))
}

pub(crate) fn pour_bottle(resolved: &ResolvedBottle) -> BrewResult<()> {
    let cellar = global::cellar_path()?;
    let rack = cellar.join(&resolved.formula.name);
    let formula_prefix = installed_prefix(&resolved.formula)?;
    let staging_root = cellar.join(format!(
        ".brew-rs-pour-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("System clock is before the Unix epoch")?
            .as_nanos()
    ));

    let result = (|| {
        fs::create_dir_all(&cellar)
            .with_context(|| format!("Failed to create {}", cellar.display()))?;
        fs::create_dir(&staging_root)
            .with_context(|| format!("Failed to create {}", staging_root.display()))?;

        formatter::ohai(format!("Pouring {}", display_bottle_basename(&resolved.bottle)?).as_str());

        let status = Command::new("tar")
            .arg("--extract")
            .arg("--no-same-owner")
            .arg("--file")
            .arg(&resolved.bottle.cached_download)
            .arg("--directory")
            .arg(&staging_root)
            .status()
            .with_context(|| {
                format!(
                    "Failed to extract {}",
                    resolved.bottle.cached_download.display()
                )
            })?;
        if !status.success() {
            bail!(
                "Failed to extract {}",
                resolved.bottle.cached_download.display()
            );
        }

        let staged_prefix = staging_root
            .join(&resolved.formula.name)
            .join(fetch::pkg_version(
                &resolved.formula.versions.stable,
                resolved.formula.revision,
            ));
        if !staged_prefix.is_dir() {
            bail!(
                "Bottle {} did not extract {}",
                resolved.bottle.formula_name,
                formula_prefix.display()
            );
        }

        fs::create_dir_all(&rack)
            .with_context(|| format!("Failed to create {}", rack.display()))?;
        fs::rename(&staged_prefix, &formula_prefix).with_context(|| {
            format!(
                "Failed to move {} to {}",
                staged_prefix.display(),
                formula_prefix.display()
            )
        })?;
        remove_install_metadata(&formula_prefix)?;
        fs::remove_dir_all(&staging_root)
            .with_context(|| format!("Failed to remove {}", staging_root.display()))?;

        Ok(())
    })();

    if result.is_err() {
        cleanup_failed_pour(&formula_prefix, &staging_root)?;
    }

    result
}

fn installed_prefix(formula: &FormulaJson) -> BrewResult<PathBuf> {
    Ok(global::cellar_path()?
        .join(&formula.name)
        .join(fetch::pkg_version(
            &formula.versions.stable,
            formula.revision,
        )))
}

fn display_bottle_basename(bottle: &BottleFetch) -> BrewResult<String> {
    let file_name = bottle
        .cached_download
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("Missing bottle filename for {}", bottle.formula_name))?;

    Ok(file_name
        .split_once("--")
        .map(|(_, basename)| basename.to_string())
        .unwrap_or_else(|| file_name.to_string()))
}

fn remove_install_metadata(formula_prefix: &Path) -> BrewResult<()> {
    for metadata_name in ["INSTALL_RECEIPT.json", "sbom.spdx.json"] {
        let metadata_path = formula_prefix.join(metadata_name);
        if metadata_path.exists() {
            fs::remove_file(&metadata_path)
                .with_context(|| format!("Failed to remove {}", metadata_path.display()))?;
        }
    }

    Ok(())
}

fn cleanup_failed_pour(formula_prefix: &Path, staging_root: &Path) -> BrewResult<()> {
    if formula_prefix.exists() {
        fs::remove_dir_all(formula_prefix)
            .with_context(|| format!("Failed to remove {}", formula_prefix.display()))?;
    }

    if staging_root.exists() {
        fs::remove_dir_all(staging_root)
            .with_context(|| format!("Failed to remove {}", staging_root.display()))?;
    }

    if let Some(rack) = formula_prefix.parent() {
        let _ = fs::remove_dir(rack);
    }

    Ok(())
}

pub(crate) fn link_installed_keg(formula_name: &str) -> BrewResult<ExitCode> {
    let status = Command::new(global::brew_file()?)
        .arg("link")
        .arg(formula_name)
        .env_remove("HOMEBREW_EXPERIMENTAL_RUST_FRONTEND")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("Failed to run `brew link`")?;

    Ok(ExitCode::from(
        status.code().unwrap_or(1).clamp(0, 255) as u8
    ))
}
